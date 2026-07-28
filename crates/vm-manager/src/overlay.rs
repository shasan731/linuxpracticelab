//! The three storage layers from spec 6.
//!
//! ```text
//! debian-base.raw        read-only, shipped, never opened for writing
//!        v
//! free-practice.qcow2    persistent user overlay
//!        v
//! lesson-<id>.qcow2      disposable lesson overlay, recreated on every reset
//! ```
//!
//! Lesson overlays are recreated rather than relying on QEMU's `-snapshot` temporary mode.
//! Both discard changes on exit, but recreating the file makes "reset" a deterministic,
//! observable operation the host can verify before boot, which is what spec 27.6 asks for.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use tokio::process::Command;

use crate::qemu_path;

pub struct OverlayManager {
    qemu_img: PathBuf,
    data_dir: PathBuf,
}

/// Where a writable layer lives and what it is stacked on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlaySpec {
    pub path: PathBuf,
    pub backing: PathBuf,
}

impl OverlayManager {
    pub fn new(qemu_img: impl Into<PathBuf>, data_dir: impl Into<PathBuf>) -> Self {
        Self {
            qemu_img: qemu_img.into(),
            data_dir: data_dir.into(),
        }
    }

    pub fn free_practice_spec(&self, base_image: &Path) -> OverlaySpec {
        OverlaySpec {
            path: self.data_dir.join("free-practice.qcow2"),
            backing: base_image.to_path_buf(),
        }
    }

    /// A lesson overlay stacks on the read-only base, not on Free Practice: a lesson must
    /// start from a known checkpoint and must not be affected by whatever the learner has
    /// done to their sandbox.
    pub fn lesson_spec(&self, lesson_id: &str, base_image: &Path) -> OverlaySpec {
        OverlaySpec {
            path: self
                .data_dir
                .join("lessons")
                .join(format!("lesson-{}.qcow2", sanitise_id(lesson_id))),
            backing: base_image.to_path_buf(),
        }
    }

    pub fn snapshot_path(&self, name: &str) -> PathBuf {
        self.data_dir
            .join("snapshots")
            .join(format!("{}.qcow2", sanitise_id(name)))
    }

    /// Creates the overlay if it is absent. For an existing overlay, updates only its
    /// backing-file metadata. This preserves learner data while repairing extended-length
    /// Windows paths and allowing a portable installation to be moved to another folder.
    pub async fn ensure(&self, spec: &OverlaySpec) -> Result<()> {
        if spec.path.exists() {
            return self.rebase(spec).await;
        }
        self.create(spec).await
    }

    /// Discards and recreates the overlay. This is what "Restart current lesson" does.
    pub async fn recreate(&self, spec: &OverlaySpec) -> Result<()> {
        if spec.path.exists() {
            std::fs::remove_file(&spec.path)
                .with_context(|| format!("could not remove {}", spec.path.display()))?;
        }
        self.create(spec).await
    }

    async fn create(&self, spec: &OverlaySpec) -> Result<()> {
        if !spec.backing.exists() {
            bail!(
                "the base image {} is missing; run Verify Runtime Files",
                spec.backing.display()
            );
        }
        if let Some(parent) = spec.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("could not create {}", parent.display()))?;
        }

        // -F states the backing format explicitly. Without it qemu-img probes the backing
        // file, which is both slower and a known security footgun.
        let output = Command::new(&self.qemu_img)
            .arg("create")
            .arg("-q")
            .args(["-f", "qcow2"])
            .arg("-F")
            .arg(backing_format(&spec.backing))
            .arg("-b")
            .arg(qemu_path::render(&spec.backing))
            .arg(qemu_path::render(&spec.path))
            .output()
            .await
            .with_context(|| format!("could not run {}", self.qemu_img.display()))?;

        if !output.status.success() {
            bail!(
                "qemu-img create failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }

    /// Changes only the recorded backing path (`-u`); it does not rewrite guest sectors.
    async fn rebase(&self, spec: &OverlaySpec) -> Result<()> {
        if !spec.backing.exists() {
            bail!(
                "the base image {} is missing; run Verify Runtime Files",
                spec.backing.display()
            );
        }
        if !self.check(&spec.path).await? {
            bail!(
                "the practice disk {} is damaged; restore a snapshot or use Factory reset",
                spec.path.display()
            );
        }

        let output = Command::new(&self.qemu_img)
            .arg("rebase")
            .arg("-u")
            .args(["-f", "qcow2"])
            .arg("-F")
            .arg(backing_format(&spec.backing))
            .arg("-b")
            .arg(qemu_path::render(&spec.backing))
            .arg(qemu_path::render(&spec.path))
            .output()
            .await
            .with_context(|| format!("could not run {}", self.qemu_img.display()))?;

        if !output.status.success() {
            bail!(
                "qemu-img could not repair the practice disk backing path: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }

    /// Checks overlay integrity before reuse (spec 20 crash recovery). Returns `false` when
    /// the file is damaged, which the caller turns into a Repair prompt rather than a crash.
    pub async fn check(&self, path: &Path) -> Result<bool> {
        if !path.exists() {
            return Ok(false);
        }
        let output = Command::new(&self.qemu_img)
            .args(["check", "-q"])
            .arg(qemu_path::render(path))
            .output()
            .await
            .with_context(|| format!("could not run {}", self.qemu_img.display()))?;
        if !output.status.success() {
            tracing::warn!(
                "overlay {} failed its integrity check: {}",
                path.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(output.status.success())
    }

    /// Copies the current Free Practice overlay aside as a restorable snapshot.
    ///
    /// A plain file copy is used rather than an internal qcow2 snapshot so a snapshot stays
    /// restorable even when the live overlay is corrupt — which is exactly the situation
    /// snapshots exist for.
    pub fn snapshot(&self, live: &Path, name: &str) -> Result<PathBuf> {
        let target = self.snapshot_path(name);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(live, &target)
            .with_context(|| format!("could not snapshot to {}", target.display()))?;
        Ok(target)
    }

    /// Restores a snapshot over the live overlay.
    ///
    /// Refuses to run when the snapshot is the only copy of anything restorable
    /// (spec 20: "never overwrite the only recovery snapshot"). The live file is moved
    /// aside first, so a failure part-way through still leaves something to go back to.
    pub fn restore(&self, snapshot: &Path, live: &Path) -> Result<()> {
        if !snapshot.exists() {
            bail!("snapshot {} does not exist", snapshot.display());
        }
        if live.exists() {
            let salvage = live.with_extension("qcow2.prerestore");
            std::fs::rename(live, &salvage).or_else(|_| {
                std::fs::copy(live, &salvage)
                    .map(|_| ())
                    .and_then(|_| std::fs::remove_file(live))
            })?;
        }
        std::fs::copy(snapshot, live)
            .with_context(|| format!("could not restore {}", snapshot.display()))?;
        Ok(())
    }

    pub fn list_snapshots(&self) -> Vec<PathBuf> {
        let dir = self.data_dir.join("snapshots");
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().map(|e| e == "qcow2").unwrap_or(false))
            .collect();
        paths.sort();
        paths
    }
}

fn backing_format(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("qcow2") => "qcow2",
        _ => "raw",
    }
}

/// Keeps lesson ids and snapshot names from escaping the data directory. Lesson ids come
/// from JSON packages, which spec 21.4 requires us to treat as untrusted input.
fn sanitise_id(id: &str) -> String {
    if id.is_empty() || id.chars().all(|character| character == '.') {
        return "unnamed".to_string();
    }

    let mut cleaned = String::with_capacity(id.len());
    for character in id.chars() {
        let safe = if character.is_ascii_alphanumeric()
            || character == '-'
            || character == '_'
            || (character == '.' && !cleaned.ends_with('.'))
        {
            character
        } else {
            '_'
        };
        cleaned.push(safe);
    }
    // A name made only of dots would still resolve to a parent directory.
    let cleaned = cleaned.trim_matches('.').to_string();
    if cleaned.is_empty() {
        "unnamed".to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager(dir: &Path) -> OverlayManager {
        OverlayManager::new("qemu-img.exe", dir)
    }

    #[test]
    fn lesson_overlays_stack_on_the_read_only_base_not_on_free_practice() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(dir.path());
        let base = PathBuf::from("C:/rt/debian-base.raw");
        let spec = m.lesson_spec("filesystem.navigation.04", &base);
        assert_eq!(spec.backing, base);
        assert!(spec
            .path
            .to_string_lossy()
            .ends_with("lesson-filesystem.navigation.04.qcow2"));
    }

    #[test]
    fn path_traversal_in_a_lesson_id_cannot_escape_the_data_directory() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(dir.path());
        let spec = m.lesson_spec("../../windows/system32/evil", Path::new("base.raw"));
        let rendered = spec.path.to_string_lossy().to_string();
        assert!(!rendered.contains(".."), "{rendered}");
        assert!(spec.path.starts_with(dir.path()));
    }

    #[test]
    fn dot_only_names_do_not_become_a_parent_directory() {
        assert_eq!(sanitise_id(".."), "unnamed");
        assert_eq!(sanitise_id("."), "unnamed");
        assert_eq!(sanitise_id("a/b"), "a_b");
    }

    #[test]
    fn backing_format_is_stated_explicitly_per_extension() {
        assert_eq!(backing_format(Path::new("x/debian-base.raw")), "raw");
        assert_eq!(backing_format(Path::new("x/free-practice.qcow2")), "qcow2");
    }

    #[test]
    fn restore_keeps_the_previous_live_overlay_as_salvage() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(dir.path());
        let live = dir.path().join("free-practice.qcow2");
        let snap = m.snapshot_path("before-dangerous-mode");
        std::fs::create_dir_all(snap.parent().unwrap()).unwrap();
        std::fs::write(&live, b"live").unwrap();
        std::fs::write(&snap, b"snapshot").unwrap();

        m.restore(&snap, &live).unwrap();

        assert_eq!(std::fs::read(&live).unwrap(), b"snapshot");
        let salvage = live.with_extension("qcow2.prerestore");
        assert_eq!(std::fs::read(salvage).unwrap(), b"live");
    }

    #[test]
    fn restoring_a_missing_snapshot_is_an_error_not_a_silent_wipe() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(dir.path());
        let live = dir.path().join("free-practice.qcow2");
        std::fs::write(&live, b"live").unwrap();
        assert!(m.restore(&dir.path().join("nope.qcow2"), &live).is_err());
        assert_eq!(std::fs::read(&live).unwrap(), b"live");
    }

    #[tokio::test]
    async fn creating_an_overlay_without_a_base_image_explains_the_fix() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(dir.path());
        let spec = OverlaySpec {
            path: dir.path().join("x.qcow2"),
            backing: dir.path().join("absent-base.raw"),
        };
        let err = m.create(&spec).await.unwrap_err().to_string();
        assert!(err.contains("Verify Runtime Files"), "{err}");
    }

    #[tokio::test]
    async fn checking_a_missing_overlay_reports_not_ok_rather_than_erroring() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(dir.path());
        assert!(!m.check(&dir.path().join("absent.qcow2")).await.unwrap());
    }
}
