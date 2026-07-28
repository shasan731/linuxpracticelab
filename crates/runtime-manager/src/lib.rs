//! Runtime layout, integrity verification and startup health.
//!
//! This crate owns the answer to "is this installation usable?" and every path the
//! application reads or writes. It contains no virtualisation and no lesson logic.

pub mod health;
pub mod install;
pub mod integrity;
pub mod layout;

pub use health::{free_disk_bytes, Finding, HealthCheck, HealthReport, RecoveryAction, Severity};
pub use install::{install_bundled_runtime, reinstall_bundled_runtime, InstallOutcome};
pub use integrity::{sha256_file, FileVerdict, Manifest, ManifestEntry, VerificationReport};
pub use layout::Layout;

/// Version of the runtime bundle this build expects, and the name of the directory it lives in.
///
/// Must equal the workspace version in Cargo.toml. The release workflow compares this against the
/// git tag and every other manifest, because a mismatch means the application looks for its
/// runtime in a directory the installer never created.
pub const RUNTIME_VERSION: &str = "0.1.0";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_version_is_a_three_part_version() {
        let parts: Vec<&str> = RUNTIME_VERSION.split('.').collect();
        assert_eq!(parts.len(), 3, "{RUNTIME_VERSION}");
        assert!(parts.iter().all(|p| p.parse::<u32>().is_ok()));
    }
}
