//! Virtual machine configuration and lifecycle state.

use crate::lesson::NetworkMode;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MachineType {
    /// Minimal x86 machine with a reduced device model and direct kernel boot (spec 3.3).
    Microvm,
    /// Fallback for anything microvm cannot express.
    Q35,
}

impl MachineType {
    pub fn as_qemu_arg(self) -> &'static str {
        match self {
            // ACPI is needed for clean shutdown via QMP. Keep the PIT enabled: without it
            // microvm exposes no usable clock-calibration source, and recent Debian kernels
            // stop during early boot before they can discover the virtio devices.
            MachineType::Microvm => "microvm,acpi=on,pit=on,pic=off,rtc=on",
            MachineType::Q35 => "q35",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AccelMode {
    /// Windows Hypervisor Platform. Used only when already enabled on the host.
    Whpx,
    /// Pure software translation. Always available, so it is the guaranteed fallback.
    Tcg,
}

impl AccelMode {
    pub fn as_qemu_arg(self) -> &'static str {
        match self {
            // kernel-irqchip=off is required for WHPX on Windows.
            AccelMode::Whpx => "whpx,kernel-irqchip=off",
            // The lab exposes one guest vCPU. QEMU's single-thread translator is both faster
            // and more reliable for that topology on Windows; multi-thread TCG can starve the
            // emulated timer during early Linux boot.
            AccelMode::Tcg => "tcg,thread=single",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            AccelMode::Whpx => "Hardware acceleration (WHPX)",
            AccelMode::Tcg => "Software translation (TCG)",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VmConfig {
    pub machine: MachineType,
    pub accel: AccelMode,
    pub cpus: u8,
    pub memory_mb: u32,
    pub kernel: PathBuf,
    pub initrd: Option<PathBuf>,
    /// Read-only base image. Never written to.
    pub base_image: PathBuf,
    /// Copy-on-write overlay backed by `base_image`.
    pub overlay: PathBuf,
    pub network_mode: NetworkMode,
    /// Loopback TCP port for QMP. Bound to 127.0.0.1 only.
    pub qmp_port: u16,
    /// Loopback TCP port carrying the guest serial console into xterm.js.
    pub console_port: u16,
    /// Loopback TCP port for the LinuxLab Agent control channel.
    pub agent_port: u16,
    /// Shared secret the guest agent requires on every control-channel request.
    pub control_token: String,
    pub log_file: PathBuf,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum VmState {
    Stopped,
    Starting,
    /// QEMU is up and the console is attached, but the agent has not answered yet.
    BootingGuest,
    /// Agent reported ready; lessons can run.
    Ready,
    Paused,
    Stopping,
    /// The guest booted at some point but is no longer usable, e.g. after the learner
    /// deliberately destroyed it in Dangerous Mode.
    Unbootable,
    Failed,
}

impl VmState {
    pub fn accepts_commands(self) -> bool {
        matches!(self, VmState::Ready)
    }

    pub fn is_transitional(self) -> bool {
        matches!(
            self,
            VmState::Starting | VmState::BootingGuest | VmState::Stopping
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VmStatus {
    pub state: VmState,
    pub accel: AccelMode,
    pub machine: MachineType,
    pub memory_mb: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boot_millis: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guest_kernel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl VmStatus {
    pub fn stopped(accel: AccelMode, machine: MachineType, memory_mb: u32) -> Self {
        Self {
            state: VmState::Stopped,
            accel,
            machine,
            memory_mb,
            pid: None,
            boot_millis: None,
            guest_kernel: None,
            image_version: None,
            detail: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_ready_accepts_commands() {
        assert!(VmState::Ready.accepts_commands());
        for s in [
            VmState::Stopped,
            VmState::Starting,
            VmState::BootingGuest,
            VmState::Paused,
            VmState::Stopping,
            VmState::Unbootable,
            VmState::Failed,
        ] {
            assert!(!s.accepts_commands(), "{s:?} must not accept commands");
        }
    }

    #[test]
    fn whpx_disables_kernel_irqchip() {
        // WHPX on Windows refuses to start with the in-kernel irqchip enabled; a regression
        // here shows up as an unexplained failure to boot on hardware-accelerated hosts.
        assert!(AccelMode::Whpx.as_qemu_arg().contains("kernel-irqchip=off"));
    }

    #[test]
    fn microvm_keeps_a_kernel_clock_source() {
        let machine = MachineType::Microvm.as_qemu_arg();
        assert!(machine.contains("pit=on"));
        assert!(!machine.contains("pit=off"));
    }

    #[test]
    fn software_translation_matches_the_single_vcpu_topology() {
        assert_eq!(AccelMode::Tcg.as_qemu_arg(), "tcg,thread=single");
    }
}
