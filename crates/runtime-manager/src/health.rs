//! Startup health check (spec 20).
//!
//! Runs before the VM starts and produces a list of findings the UI can show verbatim. The
//! design rule is that a finding either has a one-click recovery action or is not blocking —
//! a health check that only says "something is wrong" is worse than none.

use crate::integrity::{Manifest, VerificationReport};
use crate::layout::Layout;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    /// The application cannot start a lab until this is resolved.
    Blocking,
    /// Startup continues, but something is degraded.
    Warning,
    Info,
}

/// The recovery controls offered in the UI (spec 6.4, 20).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecoveryAction {
    None,
    VerifyRuntimeFiles,
    ReinstallRuntime,
    RepairUserOverlay,
    ResetPracticeEnvironment,
    RestoreLastSnapshot,
    FreeDiskSpace,
    ExportDiagnosticReport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub severity: Severity,
    pub title: String,
    pub detail: String,
    pub action: RecoveryAction,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthReport {
    pub findings: Vec<Finding>,
    /// Set when the previous run did not shut down cleanly.
    pub unclean_shutdown: bool,
    pub free_disk_bytes: Option<u64>,
}

impl HealthReport {
    pub fn can_start(&self) -> bool {
        !self
            .findings
            .iter()
            .any(|f| f.severity == Severity::Blocking)
    }

    pub fn blocking(&self) -> Vec<&Finding> {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Blocking)
            .collect()
    }
}

/// Minimum free space before we refuse to start. An overlay that fills the disk mid-lesson
/// corrupts itself, so this is checked up front rather than discovered later.
pub const MIN_FREE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

pub struct HealthCheck<'a> {
    layout: &'a Layout,
}

impl<'a> HealthCheck<'a> {
    pub fn new(layout: &'a Layout) -> Self {
        Self { layout }
    }

    /// The full startup sequence. `free_bytes` is injected so this is testable without a
    /// platform disk query.
    pub fn run(&self, free_bytes: Option<u64>, quick: bool) -> HealthReport {
        self.run_with_unclean_shutdown(free_bytes, quick, self.layout.session_lock().exists())
    }

    /// Runs the health check with the launch-time session state supplied by the application.
    ///
    /// The desktop process writes its own lock before it loads the UI. Re-reading the lock
    /// during bootstrap would therefore label every healthy current run as a previous crash.
    pub fn run_with_unclean_shutdown(
        &self,
        free_bytes: Option<u64>,
        quick: bool,
        unclean_shutdown: bool,
    ) -> HealthReport {
        let mut report = HealthReport {
            free_disk_bytes: free_bytes,
            ..Default::default()
        };

        // 1-2. Runtime files and checksums.
        match Manifest::load(&self.layout.checksums_file()) {
            Ok(manifest) => {
                let verification = if quick {
                    manifest.verify_quick(&self.layout.runtime_root)
                } else {
                    manifest.verify(&self.layout.runtime_root)
                };
                self.add_verification_findings(&mut report, &verification);
            }
            Err(err) => report.findings.push(Finding {
                severity: Severity::Blocking,
                title: "The Linux runtime is not installed correctly".into(),
                detail: format!(
                    "The runtime manifest could not be read ({err}). Reinstalling the runtime \
                     restores it without affecting your progress."
                ),
                action: RecoveryAction::ReinstallRuntime,
            }),
        }

        // 3. Disk space.
        if let Some(free) = free_bytes {
            if free < MIN_FREE_BYTES {
                report.findings.push(Finding {
                    severity: Severity::Blocking,
                    title: "Not enough free disk space".into(),
                    detail: format!(
                        "Linux Practice Lab needs about {} GB free to run safely, and there is \
                         {} GB available. Free some space and try again.",
                        MIN_FREE_BYTES / 1_073_741_824,
                        free / 1_073_741_824
                    ),
                    action: RecoveryAction::FreeDiskSpace,
                });
            }
        }

        // 4-5. Unclean shutdown and overlay consistency.
        if unclean_shutdown {
            report.unclean_shutdown = true;
            report.findings.push(Finding {
                severity: Severity::Warning,
                title: "The previous session did not close cleanly".into(),
                detail: "Your Free Practice environment will be checked before it is reused. \
                         Nothing on Windows was affected."
                    .into(),
                action: RecoveryAction::RepairUserOverlay,
            });
        }

        report
    }

    pub fn has_session_lock(&self) -> bool {
        self.layout.session_lock().exists()
    }

    fn add_verification_findings(
        &self,
        report: &mut HealthReport,
        verification: &VerificationReport,
    ) {
        for problem in verification.problems() {
            report.findings.push(Finding {
                severity: Severity::Blocking,
                title: "A Linux runtime file is damaged or missing".into(),
                detail: problem,
                action: RecoveryAction::ReinstallRuntime,
            });
        }
    }

    /// Writes the session lock. Presence of this file on the next launch means the previous
    /// run was killed.
    pub fn acquire_session_lock(&self) -> std::io::Result<()> {
        if let Some(parent) = self.layout.session_lock().parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(
            self.layout.session_lock(),
            format!("pid={}", std::process::id()),
        )
    }

    pub fn release_session_lock(&self) {
        std::fs::remove_file(self.layout.session_lock()).ok();
    }
}

/// Bytes available to *this user* on the volume holding `path`.
///
/// Reports the caller's quota-adjusted figure rather than the raw volume free space, because
/// on a managed school or office machine those two numbers differ and the one that matters is
/// what we are actually allowed to write. Returns `None` when the query fails, which callers
/// treat as "unknown" and never as a reason to block.
#[cfg(windows)]
pub fn free_disk_bytes(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    // The directory must exist for the query to resolve; walk up until one does.
    let mut probe = path.to_path_buf();
    while !probe.exists() {
        if !probe.pop() {
            return None;
        }
    }

    let wide: Vec<u16> = probe
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut available_to_caller: u64 = 0;
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut available_to_caller,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(available_to_caller)
}

#[cfg(not(windows))]
pub fn free_disk_bytes(_path: &Path) -> Option<u64> {
    // Developer and CI hosts: unknown, therefore never blocking.
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrity::{sha256_file, ManifestEntry};

    fn layout_with_intact_runtime(root: &Path) -> Layout {
        let layout = Layout::for_test(root, "1.0.0");
        std::fs::create_dir_all(&layout.runtime_root).unwrap();
        layout.ensure_writable_dirs().unwrap();

        let kernel = layout.runtime_root.join("vmlinuz");
        std::fs::write(&kernel, b"kernel").unwrap();
        let manifest = Manifest {
            runtime_version: "1.0.0".into(),
            qemu_version: "9.1.0".into(),
            image_version: "debian-13.6-1".into(),
            files: vec![ManifestEntry {
                path: "vmlinuz".into(),
                sha256: sha256_file(&kernel).unwrap(),
                size_bytes: 6,
                optional: false,
            }],
        };
        std::fs::write(
            layout.checksums_file(),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        layout
    }

    #[test]
    fn a_healthy_install_can_start() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout_with_intact_runtime(dir.path());
        let report = HealthCheck::new(&layout).run(Some(50 * 1024 * 1024 * 1024), false);
        assert!(report.can_start(), "{:?}", report.findings);
        assert!(!report.unclean_shutdown);
    }

    #[test]
    fn a_missing_manifest_blocks_startup_with_a_reinstall_action() {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::for_test(dir.path(), "1.0.0");
        layout.ensure_writable_dirs().unwrap();

        let report = HealthCheck::new(&layout).run(None, true);
        assert!(!report.can_start());
        assert_eq!(
            report.blocking()[0].action,
            RecoveryAction::ReinstallRuntime
        );
    }

    #[test]
    fn a_damaged_runtime_file_blocks_startup() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout_with_intact_runtime(dir.path());
        std::fs::write(layout.runtime_root.join("vmlinuz"), b"tamper").unwrap();

        let report = HealthCheck::new(&layout).run(None, false);
        assert!(!report.can_start());
    }

    #[test]
    fn low_disk_space_blocks_startup_and_says_the_numbers() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout_with_intact_runtime(dir.path());
        let report = HealthCheck::new(&layout).run(Some(100 * 1024 * 1024), false);

        assert!(!report.can_start());
        let finding = report
            .blocking()
            .into_iter()
            .find(|f| f.action == RecoveryAction::FreeDiskSpace)
            .expect("expected a disk space finding");
        assert!(finding.detail.contains("2 GB free"), "{}", finding.detail);
        assert!(
            finding.detail.contains("0 GB available"),
            "{}",
            finding.detail
        );
    }

    #[test]
    fn unknown_free_space_does_not_block() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout_with_intact_runtime(dir.path());
        assert!(HealthCheck::new(&layout).run(None, false).can_start());
    }

    #[test]
    fn a_leftover_session_lock_warns_but_does_not_block() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout_with_intact_runtime(dir.path());
        let check = HealthCheck::new(&layout);
        check.acquire_session_lock().unwrap();

        let report = check.run(None, false);
        assert!(report.unclean_shutdown);
        assert!(report.can_start(), "a crash must not lock the learner out");
        assert_eq!(report.findings[0].severity, Severity::Warning);

        check.release_session_lock();
        assert!(!check.run(None, false).unclean_shutdown);
    }

    #[test]
    fn the_current_process_lock_is_not_reported_as_a_previous_crash() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout_with_intact_runtime(dir.path());
        let check = HealthCheck::new(&layout);
        check.acquire_session_lock().unwrap();

        let report = check.run_with_unclean_shutdown(None, false, false);
        assert!(!report.unclean_shutdown);
        assert!(report
            .findings
            .iter()
            .all(|finding| finding.action != RecoveryAction::RepairUserOverlay));
    }

    #[test]
    fn every_blocking_finding_offers_a_recovery_action() {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::for_test(dir.path(), "1.0.0");
        layout.ensure_writable_dirs().unwrap();
        let report = HealthCheck::new(&layout).run(Some(0), false);

        assert!(!report.blocking().is_empty());
        for finding in report.blocking() {
            assert_ne!(
                finding.action,
                RecoveryAction::None,
                "blocking finding without a fix: {}",
                finding.title
            );
        }
    }
}
