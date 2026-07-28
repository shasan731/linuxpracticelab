//! Host-side virtual machine orchestration.
//!
//! Responsibilities: decide how to launch QEMU, launch it in a way that cannot outlive the
//! application, keep the storage overlays consistent, and expose lifecycle control. It knows
//! nothing about lessons or progress.

pub mod accel;
pub mod overlay;
pub mod process;
pub mod qemu;
mod qemu_path;
pub mod qmp;

use anyhow::{Context, Result};
use rand::Rng;
use shared_types::{MachineType, NetworkMode, VmConfig, VmState, VmStatus};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const DEFAULT_MEMORY_MB: u32 = 256;
pub const ADVANCED_MEMORY_MB: u32 = 384;

/// Paths the manager needs from the runtime layout.
#[derive(Debug, Clone)]
pub struct RuntimePaths {
    pub qemu_system: PathBuf,
    pub qemu_img: PathBuf,
    pub kernel: PathBuf,
    pub initrd: Option<PathBuf>,
    pub base_image: PathBuf,
    pub data_dir: PathBuf,
    pub log_dir: PathBuf,
}

impl RuntimePaths {
    pub fn vm_log(&self) -> PathBuf {
        self.log_dir.join("vm.log")
    }
}

/// Which writable layer a session runs against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionKind {
    FreePractice,
    Lesson { lesson_id: String },
}

pub struct VmManager {
    paths: RuntimePaths,
    overlays: overlay::OverlayManager,
    accel: accel::AccelDecision,
    machine: MachineType,
    supervised: Option<process::Supervised>,
    config: Option<VmConfig>,
    state: VmState,
    started_at: Option<Instant>,
    boot_millis: Option<u64>,
    detail: Option<String>,
}

impl VmManager {
    pub fn new(paths: RuntimePaths) -> Self {
        let overlays = overlay::OverlayManager::new(paths.qemu_img.clone(), paths.data_dir.clone());
        let accel = accel::detect();
        let machine = preferred_machine(accel.mode);
        Self {
            paths,
            overlays,
            accel,
            machine,
            supervised: None,
            config: None,
            state: VmState::Stopped,
            started_at: None,
            boot_millis: None,
            detail: None,
        }
    }

    pub fn accel_decision(&self) -> &accel::AccelDecision {
        &self.accel
    }

    pub fn overlays(&self) -> &overlay::OverlayManager {
        &self.overlays
    }

    pub fn state(&self) -> VmState {
        self.state
    }

    pub fn status(&self) -> VmStatus {
        VmStatus {
            state: self.state,
            accel: self.accel.mode,
            machine: self.machine,
            memory_mb: self
                .config
                .as_ref()
                .map(|c| c.memory_mb)
                .unwrap_or(DEFAULT_MEMORY_MB),
            pid: self.supervised.as_ref().map(|s| s.pid),
            boot_millis: self.boot_millis,
            guest_kernel: None,
            image_version: None,
            detail: self.detail.clone(),
        }
    }

    pub fn config(&self) -> Option<&VmConfig> {
        self.config.as_ref()
    }

    /// Falls back to the q35 machine type for the rest of the session. Called when a microvm
    /// boot fails, so a compatibility problem degrades instead of blocking the learner.
    pub fn use_fallback_machine(&mut self) {
        tracing::warn!("falling back to the q35 machine type");
        self.machine = MachineType::Q35;
    }

    /// Falls back when WHPX is installed but cannot run this QEMU/host combination.
    ///
    /// The Windows capability API only proves that a hypervisor is present. Firmware policy,
    /// another hypervisor, or a QEMU regression can still make the first virtual CPU hang.
    /// Software translation is slower but is the compatibility floor promised by the app.
    pub fn use_software_acceleration(&mut self, detail: &str) {
        tracing::warn!("falling back to software translation: {detail}");
        self.accel = accel::AccelDecision {
            mode: shared_types::AccelMode::Tcg,
            reason: format!("Running with software translation ({detail})."),
        };
        // QEMU's microvm machine intermittently stalls before the kernel's first instruction
        // under Windows TCG. q35 is slightly heavier but is the reliable compatibility floor.
        self.machine = preferred_machine(self.accel.mode);
    }

    /// Removes a QEMU left behind by a crash, before anything else touches the overlays.
    pub fn reap_stale_process(&self) -> Result<bool> {
        let pid_file = process::PidFile::new(&self.paths.data_dir);
        let Some(pid) = pid_file.read() else {
            return Ok(false);
        };
        let killed = process::kill_stale(pid, &self.paths.qemu_system)?;
        pid_file.clear();
        Ok(killed)
    }

    /// Assembles the launch configuration for a session, creating overlays as needed.
    pub async fn prepare(
        &mut self,
        kind: &SessionKind,
        network_mode: NetworkMode,
        memory_mb: Option<u32>,
    ) -> Result<VmConfig> {
        let spec = match kind {
            SessionKind::FreePractice => self.overlays.free_practice_spec(&self.paths.base_image),
            SessionKind::Lesson { lesson_id } => {
                self.overlays.lesson_spec(lesson_id, &self.paths.base_image)
            }
        };

        match kind {
            // The learner's sandbox persists across restarts.
            SessionKind::FreePractice => self.overlays.ensure(&spec).await?,
            // A lesson always starts from the known checkpoint.
            SessionKind::Lesson { .. } => self.overlays.recreate(&spec).await?,
        }

        let config = VmConfig {
            machine: self.machine,
            accel: self.accel.mode,
            cpus: 1,
            memory_mb: memory_mb.unwrap_or(DEFAULT_MEMORY_MB),
            kernel: self.paths.kernel.clone(),
            initrd: self.paths.initrd.clone(),
            base_image: self.paths.base_image.clone(),
            overlay: spec.path,
            network_mode,
            qmp_port: reserve_loopback_port()?,
            console_port: reserve_loopback_port()?,
            agent_port: reserve_loopback_port()?,
            control_token: generate_token(),
            log_file: self.paths.vm_log(),
        };
        self.config = Some(config.clone());
        Ok(config)
    }

    /// Launches QEMU. The caller then connects the console and waits for the agent.
    pub async fn start(&mut self, config: &VmConfig) -> Result<VmStatus> {
        if self.supervised.is_some() {
            self.poll_exited()?;
        }
        if self.supervised.is_some() {
            anyhow::bail!("a virtual machine is already running");
        }
        std::fs::create_dir_all(&self.paths.log_dir).ok();

        let args = qemu::build_args(config);
        tracing::info!("launching qemu: {}", qemu::describe(config));

        self.state = VmState::Starting;
        self.started_at = Some(Instant::now());

        let working_dir = self
            .paths
            .qemu_system
            .parent()
            .unwrap_or(Path::new("."))
            .to_path_buf();

        let supervised = process::spawn(
            &self.paths.qemu_system,
            &args,
            &working_dir,
            &self.paths.vm_log(),
        )
        .await?;

        process::PidFile::new(&self.paths.data_dir)
            .write(supervised.pid)
            .ok();
        self.supervised = Some(supervised);
        self.state = VmState::BootingGuest;
        Ok(self.status())
    }

    /// Called once the guest agent answers a ping.
    pub fn mark_ready(&mut self) {
        if let Some(started) = self.started_at {
            self.boot_millis = Some(started.elapsed().as_millis() as u64);
        }
        self.state = VmState::Ready;
        self.detail = None;
    }

    /// Called when QEMU exits unexpectedly. A guest that panicked itself into oblivion is
    /// reported as `Unbootable` so the UI can offer snapshot restore rather than a raw error.
    pub fn mark_exited(&mut self, guest_had_booted: bool) {
        self.supervised = None;
        process::PidFile::new(&self.paths.data_dir).clear();
        self.state = if guest_had_booted {
            self.detail = Some(
                "The Linux practice environment is no longer bootable. Windows was not affected."
                    .into(),
            );
            VmState::Unbootable
        } else {
            self.detail =
                Some("The virtual machine stopped before Linux finished starting.".into());
            VmState::Failed
        };
    }

    /// Non-blocking child-process check used while the guest agent is still booting.
    ///
    /// Without this, an invalid QEMU command line looks like a two-minute guest timeout even
    /// though the process exited immediately and already wrote the real reason to `vm.log`.
    pub fn poll_exited(&mut self) -> Result<Option<std::process::ExitStatus>> {
        let status = match self.supervised.as_mut() {
            Some(supervised) => supervised
                .child
                .try_wait()
                .context("could not check the virtual machine process")?,
            None => return Ok(None),
        };
        if status.is_some() {
            self.mark_exited(false);
        }
        Ok(status)
    }

    pub fn mark_failed(&mut self, detail: impl Into<String>) {
        self.state = VmState::Failed;
        self.detail = Some(detail.into());
    }

    /// Polite shutdown first, then the hammer. Every writable layer is disposable, so the
    /// hammer is safe; we try ACPI first only to let systemd flush the journal.
    pub async fn stop(&mut self) -> Result<()> {
        let Some(config) = self.config.clone() else {
            return Ok(());
        };
        if self.supervised.is_none() {
            return Ok(());
        }
        let guest_was_ready = matches!(self.state, VmState::Ready | VmState::Paused);
        self.state = VmState::Stopping;

        // A guest that never became ready cannot handle ACPI reliably. Terminate it
        // immediately so a failed start does not consume a CPU or block the next attempt.
        if guest_was_ready {
            if let Ok(mut client) =
                qmp::QmpClient::connect(config.qmp_port, Duration::from_millis(1500)).await
            {
                client.system_powerdown().await.ok();
                let deadline = Instant::now() + Duration::from_secs(8);
                while Instant::now() < deadline {
                    match self.supervised.as_mut().map(|s| s.child.try_wait()) {
                        Some(Ok(Some(_))) | None => break,
                        _ => tokio::time::sleep(Duration::from_millis(200)).await,
                    }
                }
                client.quit().await.ok();
            }
        }

        if let Some(supervised) = self.supervised.as_mut() {
            supervised.terminate().await?;
        }
        self.supervised = None;
        process::PidFile::new(&self.paths.data_dir).clear();
        self.state = VmState::Stopped;
        self.boot_millis = None;
        Ok(())
    }

    pub async fn pause(&mut self) -> Result<()> {
        let config = self
            .config
            .clone()
            .context("no virtual machine configured")?;
        let mut client = qmp::QmpClient::connect(config.qmp_port, Duration::from_secs(2)).await?;
        client.pause().await?;
        self.state = VmState::Paused;
        Ok(())
    }

    pub async fn resume(&mut self) -> Result<()> {
        let config = self
            .config
            .clone()
            .context("no virtual machine configured")?;
        let mut client = qmp::QmpClient::connect(config.qmp_port, Duration::from_secs(2)).await?;
        client.resume().await?;
        self.state = VmState::Ready;
        Ok(())
    }
}

fn preferred_machine(accel: shared_types::AccelMode) -> MachineType {
    match accel {
        shared_types::AccelMode::Tcg => MachineType::Q35,
        shared_types::AccelMode::Whpx => MachineType::Microvm,
    }
}

/// Picks a free loopback port by binding one and releasing it.
///
/// There is an inherent race between releasing and QEMU binding, so QEMU's bind failure is
/// still handled by the caller; this only avoids the common collision.
fn reserve_loopback_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .context("could not reserve a loopback port for the virtual machine control channel")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

/// 256 bits of randomness, hex encoded. Guards the agent control channel for one VM run.
fn generate_token() -> String {
    use std::fmt::Write;

    let mut bytes = [0u8; 32];
    rand::thread_rng().fill(&mut bytes);
    bytes.iter().fold(
        String::with_capacity(bytes.len() * 2),
        |mut encoded, byte| {
            write!(encoded, "{byte:02x}").expect("writing to a string cannot fail");
            encoded
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared_types::AccelMode;

    #[test]
    fn tokens_are_long_and_unique_per_run() {
        let a = generate_token();
        let b = generate_token();
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn reserved_ports_are_usable_and_distinct() {
        let a = reserve_loopback_port().unwrap();
        let b = reserve_loopback_port().unwrap();
        assert!(a >= 1024 && b >= 1024);
        assert_ne!(a, b);
    }

    #[test]
    fn default_memory_matches_the_performance_budget() {
        assert_eq!(DEFAULT_MEMORY_MB, 256);
        assert_eq!(ADVANCED_MEMORY_MB, 384);
    }

    #[test]
    fn accel_never_blocks_startup() {
        // Whatever the host looks like, we always end up with a launchable mode.
        let decision = accel::detect();
        assert!(matches!(decision.mode, AccelMode::Whpx | AccelMode::Tcg));
    }

    #[test]
    fn software_translation_uses_the_reliable_q35_machine() {
        assert_eq!(preferred_machine(AccelMode::Tcg), MachineType::Q35);
        assert_eq!(preferred_machine(AccelMode::Whpx), MachineType::Microvm);
    }
}
