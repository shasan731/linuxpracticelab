//! Install and data layout (spec 4.2).
//!
//! Everything lives under `%LOCALAPPDATA%`, which is writable by a standard user. No path in
//! here requires administrator rights, and none of them is under Program Files.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};

/// `%LOCALAPPDATA%\LinuxPracticeLab`
pub const DATA_DIR_NAME: &str = "LinuxPracticeLab";
/// `%LOCALAPPDATA%\Programs\LinuxPracticeLab`
pub const INSTALL_DIR_NAME: &str = "Programs";

#[derive(Debug, Clone)]
pub struct Layout {
    /// Root of user data: overlays, progress database, snapshots, logs.
    pub data_root: PathBuf,
    /// Root of the versioned runtime: QEMU, kernel, base image, licences.
    pub runtime_root: PathBuf,
    /// Read-only copy of the compressed runtime shipped as an application resource.
    pub bundled_runtime_root: PathBuf,
    /// Lesson packages shipped with the application.
    pub lessons_root: PathBuf,
    pub runtime_version: String,
}

impl Layout {
    /// Derives the layout from the environment. Honours `LINUXLAB_DATA_DIR` so a portable
    /// build can keep its data beside the executable.
    pub fn discover(runtime_version: &str, resource_dir: &Path) -> Result<Self> {
        Self::discover_with_data_dir(
            runtime_version,
            resource_dir,
            std::env::var("LINUXLAB_DATA_DIR")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .map(PathBuf::from),
        )
    }

    fn discover_with_data_dir(
        runtime_version: &str,
        resource_dir: &Path,
        explicit_data_root: Option<PathBuf>,
    ) -> Result<Self> {
        let data_root = match explicit_data_root {
            Some(value) => value,
            _ if resource_dir.join("portable.mode").is_file() => resource_dir.join("portable-data"),
            _ => local_app_data()?.join(DATA_DIR_NAME),
        };
        Ok(Self {
            data_root: data_root.clone(),
            runtime_root: data_root.join("runtime").join(runtime_version),
            bundled_runtime_root: resource_dir.join("runtime").join(runtime_version),
            lessons_root: resource_dir.join("lessons"),
            runtime_version: runtime_version.to_string(),
        })
    }

    pub fn for_test(root: &Path, runtime_version: &str) -> Self {
        Self {
            data_root: root.join("data"),
            runtime_root: root.join("runtime").join(runtime_version),
            bundled_runtime_root: root.join("bundled-runtime").join(runtime_version),
            lessons_root: root.join("lessons"),
            runtime_version: runtime_version.to_string(),
        }
    }

    pub fn qemu_system(&self) -> PathBuf {
        self.runtime_root.join(qemu_system_filename())
    }

    pub fn qemu_img(&self) -> PathBuf {
        self.runtime_root.join(qemu_img_filename())
    }

    pub fn kernel(&self) -> PathBuf {
        self.runtime_root.join("vmlinuz")
    }

    pub fn initrd(&self) -> PathBuf {
        self.runtime_root.join("initrd.img")
    }

    /// The decompressed read-only base image. Shipped as `.raw.zst` and expanded on first run
    /// so the installer stays inside its size budget.
    pub fn base_image(&self) -> PathBuf {
        self.runtime_root.join("debian-base.raw")
    }

    pub fn base_image_compressed(&self) -> PathBuf {
        self.runtime_root.join("debian-base.raw.zst")
    }

    pub fn licences_dir(&self) -> PathBuf {
        self.runtime_root.join("licences")
    }

    pub fn checksums_file(&self) -> PathBuf {
        self.runtime_root.join("checksums.json")
    }

    pub fn user_data_dir(&self) -> PathBuf {
        self.data_root.join("data")
    }

    pub fn progress_db(&self) -> PathBuf {
        self.user_data_dir().join("progress.db")
    }

    pub fn settings_file(&self) -> PathBuf {
        self.user_data_dir().join("settings.json")
    }

    pub fn free_practice_overlay(&self) -> PathBuf {
        self.user_data_dir().join("free-practice.qcow2")
    }

    pub fn snapshots_dir(&self) -> PathBuf {
        self.user_data_dir().join("snapshots")
    }

    pub fn lesson_cache_dir(&self) -> PathBuf {
        self.user_data_dir().join("lesson-cache")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.data_root.join("logs")
    }

    pub fn application_log(&self) -> PathBuf {
        self.logs_dir().join("application.log")
    }

    pub fn vm_log(&self) -> PathBuf {
        self.logs_dir().join("vm.log")
    }

    /// Lock file proving only one instance is running. Its presence at startup means the
    /// previous run did not exit cleanly.
    pub fn session_lock(&self) -> PathBuf {
        self.data_root.join("session.lock")
    }

    /// Creates the directories the application writes to. Runtime directories are created by
    /// the installer, not here.
    pub fn ensure_writable_dirs(&self) -> Result<()> {
        for dir in [
            self.user_data_dir(),
            self.snapshots_dir(),
            self.lesson_cache_dir(),
            self.logs_dir(),
            self.user_data_dir().join("lessons"),
        ] {
            std::fs::create_dir_all(&dir)
                .with_context(|| format!("could not create {}", dir.display()))?;
        }
        Ok(())
    }

    /// Guards against a lesson id or snapshot name escaping the data directory.
    pub fn contains(&self, path: &Path) -> bool {
        let Ok(canonical_root) = self.data_root.canonicalize() else {
            return false;
        };
        match path.canonicalize() {
            Ok(canonical) => canonical.starts_with(&canonical_root),
            // Not created yet: fall back to a lexical check after removing `.` components.
            Err(_) => {
                let mut normalised = PathBuf::new();
                for component in path.components() {
                    match component {
                        std::path::Component::ParentDir => {
                            normalised.pop();
                        }
                        std::path::Component::CurDir => {}
                        other => normalised.push(other),
                    }
                }
                normalised.starts_with(&self.data_root) || normalised.starts_with(&canonical_root)
            }
        }
    }
}

fn local_app_data() -> Result<PathBuf> {
    std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|_| {
            // Developer hosts that are not Windows.
            std::env::var("XDG_DATA_HOME").map(PathBuf::from)
        })
        .or_else(|_| std::env::var("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .map_err(|_| anyhow!("could not determine a writable application data directory"))
}

pub fn qemu_system_filename() -> &'static str {
    if cfg!(windows) {
        "qemu-system-x86_64.exe"
    } else {
        "qemu-system-x86_64"
    }
}

pub fn qemu_img_filename() -> &'static str {
    if cfg!(windows) {
        "qemu-img.exe"
    } else {
        "qemu-img"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_in_the_layout_needs_administrator_rights() {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::for_test(dir.path(), "1.0.0");
        for path in [
            layout.progress_db(),
            layout.free_practice_overlay(),
            layout.application_log(),
            layout.snapshots_dir(),
        ] {
            let rendered = path.to_string_lossy().to_lowercase();
            assert!(!rendered.contains("program files"), "{rendered}");
            assert!(!rendered.contains("system32"), "{rendered}");
        }
    }

    #[test]
    fn runtime_is_versioned_so_upgrades_do_not_overwrite_a_running_install() {
        let dir = tempfile::tempdir().unwrap();
        let a = Layout::for_test(dir.path(), "1.0.0");
        let b = Layout::for_test(dir.path(), "1.1.0");
        assert_ne!(a.runtime_root, b.runtime_root);
        assert!(a.qemu_system().to_string_lossy().contains("1.0.0"));
        // User data is shared across runtime versions: progress must survive an upgrade.
        assert_eq!(a.progress_db(), b.progress_db());
    }

    #[test]
    fn writable_directories_are_created() {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::for_test(dir.path(), "1.0.0");
        layout.ensure_writable_dirs().unwrap();
        assert!(layout.user_data_dir().is_dir());
        assert!(layout.snapshots_dir().is_dir());
        assert!(layout.logs_dir().is_dir());
    }

    #[test]
    fn containment_check_rejects_traversal_outside_the_data_root() {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::for_test(dir.path(), "1.0.0");
        layout.ensure_writable_dirs().unwrap();

        assert!(layout.contains(&layout.snapshots_dir().join("a.qcow2")));
        assert!(!layout.contains(&layout.data_root.join("../../elsewhere/evil.qcow2")));
        assert!(!layout.contains(Path::new("C:/Windows/System32/config")));
    }

    #[test]
    fn base_image_is_shipped_compressed_and_expanded_beside_it() {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::for_test(dir.path(), "1.0.0");
        assert_eq!(
            layout.base_image_compressed().parent(),
            layout.base_image().parent()
        );
        assert!(layout
            .base_image_compressed()
            .to_string_lossy()
            .ends_with(".raw.zst"));
    }

    #[test]
    fn an_explicit_data_dir_is_honoured_for_portable_builds() {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::discover_with_data_dir(
            "1.0.0",
            Path::new("resources"),
            Some(dir.path().to_path_buf()),
        )
        .unwrap();
        assert_eq!(layout.data_root, dir.path());
    }

    #[test]
    fn a_portable_marker_keeps_data_beside_the_application() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("portable.mode"), b"").unwrap();

        let layout = Layout::discover_with_data_dir("1.0.0", dir.path(), None).unwrap();

        assert_eq!(layout.data_root, dir.path().join("portable-data"));
    }
}
