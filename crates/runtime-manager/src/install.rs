//! First-run installation of the bundled Linux runtime.
//!
//! Tauri resources are read-only and the guest base image is shipped compressed. The virtual
//! machine needs an ordinary raw image in a user-writable directory, so startup copies the
//! versioned payload to a staging directory, verifies every packaged byte, decompresses the
//! image, verifies the materialised image, and only then makes the directory live.

use crate::{Layout, Manifest};
use anyhow::{bail, Context, Result};
use std::fs::{self, File};
use std::path::Path;

const RAW_IMAGE: &str = "debian-base.raw";
const COMPRESSED_IMAGE: &str = "debian-base.raw.zst";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    AlreadyInstalled,
    Installed,
}

pub fn install_bundled_runtime(layout: &Layout) -> Result<InstallOutcome> {
    install_runtime(layout, false)
}

pub fn reinstall_bundled_runtime(layout: &Layout) -> Result<InstallOutcome> {
    install_runtime(layout, true)
}

fn install_runtime(layout: &Layout, force: bool) -> Result<InstallOutcome> {
    if !force && installed_runtime_is_complete(layout) {
        return Ok(InstallOutcome::AlreadyInstalled);
    }

    if !layout.bundled_runtime_root.is_dir() {
        bail!(
            "the bundled runtime is missing at {}; reinstall Linux Practice Lab",
            layout.bundled_runtime_root.display()
        );
    }

    let runtime_parent = layout
        .runtime_root
        .parent()
        .context("the runtime directory has no parent")?;
    fs::create_dir_all(runtime_parent)
        .with_context(|| format!("could not create {}", runtime_parent.display()))?;

    let staging = runtime_parent.join(format!(".installing-{}", std::process::id()));
    remove_staging_dir(&staging, runtime_parent)?;

    let result = stage_and_verify(layout, &staging);
    if let Err(error) = result {
        remove_staging_dir(&staging, runtime_parent).ok();
        return Err(error);
    }

    if layout.runtime_root.exists() {
        ensure_direct_child(&layout.runtime_root, runtime_parent)?;
        refuse_link(&layout.runtime_root)?;
        fs::remove_dir_all(&layout.runtime_root).with_context(|| {
            format!(
                "could not replace incomplete runtime {}",
                layout.runtime_root.display()
            )
        })?;
    }
    fs::rename(&staging, &layout.runtime_root).with_context(|| {
        format!(
            "could not activate runtime {}",
            layout.runtime_root.display()
        )
    })?;

    Ok(InstallOutcome::Installed)
}

fn installed_runtime_is_complete(layout: &Layout) -> bool {
    let Ok(manifest) = Manifest::load(&layout.checksums_file()) else {
        return false;
    };
    manifest.runtime_version == layout.runtime_version
        && manifest.verify_quick(&layout.runtime_root).is_healthy()
}

fn stage_and_verify(layout: &Layout, staging: &Path) -> Result<()> {
    copy_directory(&layout.bundled_runtime_root, staging)?;

    let manifest_path = staging.join("checksums.json");
    let manifest = Manifest::load(&manifest_path)?;
    if manifest.runtime_version != layout.runtime_version {
        bail!(
            "the bundled runtime is version {}, but the application requires {}",
            manifest.runtime_version,
            layout.runtime_version
        );
    }

    let raw_entry = manifest
        .files
        .iter()
        .find(|entry| entry.path == RAW_IMAGE)
        .cloned()
        .context("the runtime manifest does not describe the materialised base image")?;
    if raw_entry.optional {
        bail!("the materialised base image cannot be optional");
    }

    let mut packaged_manifest = manifest.clone();
    packaged_manifest
        .files
        .retain(|entry| entry.path != RAW_IMAGE);
    let packaged_report = packaged_manifest.verify(staging);
    if !packaged_report.is_healthy() {
        bail!(
            "the bundled runtime failed verification: {}",
            packaged_report.problems().join(" ")
        );
    }

    let compressed = staging.join(COMPRESSED_IMAGE);
    let raw = staging.join(RAW_IMAGE);
    let input = File::open(&compressed)
        .with_context(|| format!("could not open {}", compressed.display()))?;
    let output =
        File::create(&raw).with_context(|| format!("could not create {}", raw.display()))?;
    zstd::stream::copy_decode(input, output).with_context(|| {
        format!(
            "could not decompress the Linux base image {}",
            compressed.display()
        )
    })?;
    fs::remove_file(&compressed).with_context(|| {
        format!(
            "could not remove compressed image after installation {}",
            compressed.display()
        )
    })?;

    let mut raw_manifest = manifest;
    raw_manifest.files = vec![raw_entry];
    let raw_report = raw_manifest.verify(staging);
    if !raw_report.is_healthy() {
        bail!(
            "the decompressed Linux base image failed verification: {}",
            raw_report.problems().join(" ")
        );
    }
    Ok(())
}

fn copy_directory(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)
        .with_context(|| format!("could not create {}", destination.display()))?;
    for entry in fs::read_dir(source)
        .with_context(|| format!("could not read bundled runtime {}", source.display()))?
    {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_directory(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), &target).with_context(|| {
                format!(
                    "could not copy runtime file {} to {}",
                    entry.path().display(),
                    target.display()
                )
            })?;
        } else {
            bail!(
                "the bundled runtime contains an unsupported link or device at {}",
                entry.path().display()
            );
        }
    }
    Ok(())
}

fn remove_staging_dir(staging: &Path, runtime_parent: &Path) -> Result<()> {
    ensure_direct_child(staging, runtime_parent)?;
    if staging.exists() {
        refuse_link(staging)?;
        fs::remove_dir_all(staging)
            .with_context(|| format!("could not clear stale staging area {}", staging.display()))?;
    }
    Ok(())
}

fn refuse_link(path: &Path) -> Result<()> {
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        bail!("refusing to remove linked runtime path {}", path.display());
    }
    Ok(())
}

fn ensure_direct_child(path: &Path, expected_parent: &Path) -> Result<()> {
    if path.parent() != Some(expected_parent) {
        bail!(
            "refusing to remove runtime path outside {}",
            expected_parent.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{sha256_file, ManifestEntry};
    use sha2::Digest;
    use std::io::Write;

    fn bundled_runtime(layout: &Layout, raw_content: &[u8]) {
        fs::create_dir_all(&layout.bundled_runtime_root).unwrap();
        fs::write(layout.bundled_runtime_root.join("qemu-img.exe"), b"qemu").unwrap();

        let compressed = layout.bundled_runtime_root.join(COMPRESSED_IMAGE);
        let mut encoder = zstd::Encoder::new(File::create(&compressed).unwrap(), 1).unwrap();
        encoder.write_all(raw_content).unwrap();
        encoder.finish().unwrap();

        let manifest = Manifest {
            runtime_version: layout.runtime_version.clone(),
            qemu_version: "9.2.0".into(),
            image_version: "test-image".into(),
            files: vec![
                ManifestEntry {
                    path: "qemu-img.exe".into(),
                    sha256: sha256_file(&layout.bundled_runtime_root.join("qemu-img.exe")).unwrap(),
                    size_bytes: 4,
                    optional: false,
                },
                ManifestEntry {
                    path: COMPRESSED_IMAGE.into(),
                    sha256: sha256_file(&compressed).unwrap(),
                    size_bytes: fs::metadata(&compressed).unwrap().len(),
                    optional: true,
                },
                ManifestEntry {
                    path: RAW_IMAGE.into(),
                    sha256: hex::encode(sha2::Sha256::digest(raw_content)),
                    size_bytes: raw_content.len() as u64,
                    optional: false,
                },
            ],
        };
        fs::write(
            layout.bundled_runtime_root.join("checksums.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn first_run_installs_and_verifies_the_materialised_image() {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::for_test(dir.path(), "1.0.0");
        bundled_runtime(&layout, b"small raw image");

        assert_eq!(
            install_bundled_runtime(&layout).unwrap(),
            InstallOutcome::Installed
        );
        assert_eq!(fs::read(layout.base_image()).unwrap(), b"small raw image");
        assert!(Manifest::load(&layout.checksums_file())
            .unwrap()
            .verify(&layout.runtime_root)
            .is_healthy());
    }

    #[test]
    fn a_complete_runtime_is_not_copied_again() {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::for_test(dir.path(), "1.0.0");
        bundled_runtime(&layout, b"raw");
        install_bundled_runtime(&layout).unwrap();

        fs::remove_dir_all(&layout.bundled_runtime_root).unwrap();
        assert_eq!(
            install_bundled_runtime(&layout).unwrap(),
            InstallOutcome::AlreadyInstalled
        );
    }

    #[test]
    fn corrupt_packaged_bytes_are_rejected_before_activation() {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::for_test(dir.path(), "1.0.0");
        bundled_runtime(&layout, b"raw");
        fs::write(layout.bundled_runtime_root.join("qemu-img.exe"), b"evil").unwrap();

        let error = install_bundled_runtime(&layout).unwrap_err().to_string();
        assert!(error.contains("failed verification"), "{error}");
        assert!(!layout.runtime_root.exists());
    }

    #[test]
    fn forced_reinstall_repairs_same_size_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::for_test(dir.path(), "1.0.0");
        bundled_runtime(&layout, b"good");
        install_bundled_runtime(&layout).unwrap();
        fs::write(layout.base_image(), b"evil").unwrap();

        assert_eq!(
            reinstall_bundled_runtime(&layout).unwrap(),
            InstallOutcome::Installed
        );
        assert_eq!(fs::read(layout.base_image()).unwrap(), b"good");
    }
}
