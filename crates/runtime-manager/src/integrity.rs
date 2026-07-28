//! Runtime file verification.
//!
//! The startup health check (spec 20) verifies that every runtime file is present and hashes
//! to what the build recorded. Antivirus quarantine, a half-finished download and a truncated
//! copy all look identical from the outside; hashing is what tells them apart, and it is why
//! "Verify Runtime Files" can give a real answer instead of a shrug.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub runtime_version: String,
    /// QEMU version string, recorded for the licence and source-offer paperwork.
    pub qemu_version: String,
    pub image_version: String,
    pub files: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// Path relative to the runtime root.
    pub path: String,
    pub sha256: String,
    pub size_bytes: u64,
    /// Optional files may be absent after installation. `initrd.img` is optional when the
    /// kernel can boot without it, and the compressed image is removed after materialisation.
    #[serde(default)]
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileVerdict {
    Ok,
    Missing,
    WrongSize { expected: u64, actual: u64 },
    WrongHash { expected: String, actual: String },
    Unreadable(String),
}

impl FileVerdict {
    pub fn is_ok(&self) -> bool {
        matches!(self, FileVerdict::Ok)
    }

    /// What to tell the learner. These are the only failure paths a normal user ever sees, so
    /// each one names the recovery action rather than the internal cause.
    pub fn explain(&self, path: &str) -> String {
        match self {
            FileVerdict::Ok => format!("{path} is intact."),
            FileVerdict::Missing => format!(
                "{path} is missing. Antivirus software sometimes removes it. Use Reinstall \
                 Runtime to restore it."
            ),
            FileVerdict::WrongSize { expected, actual } => format!(
                "{path} is {actual} bytes but should be {expected}. The copy is incomplete; use \
                 Reinstall Runtime."
            ),
            FileVerdict::WrongHash { .. } => format!(
                "{path} does not match the version that was installed. Use Reinstall Runtime."
            ),
            FileVerdict::Unreadable(reason) => format!(
                "{path} could not be read ({reason}). Check that another program is not \
                 locking it."
            ),
        }
    }
}

#[derive(Debug, Clone)]
pub struct VerificationReport {
    pub results: Vec<(String, FileVerdict)>,
}

impl VerificationReport {
    pub fn is_healthy(&self) -> bool {
        self.results.iter().all(|(_, verdict)| verdict.is_ok())
    }

    pub fn problems(&self) -> Vec<String> {
        self.results
            .iter()
            .filter(|(_, verdict)| !verdict.is_ok())
            .map(|(path, verdict)| verdict.explain(path))
            .collect()
    }
}

impl Manifest {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("could not read the runtime manifest {}", path.display()))?;
        // Windows PowerShell 5 writes a UTF-8 BOM for `-Encoding utf8`. Release packaging
        // normally emits BOM-free JSON, but accepting the marker makes runtime recovery robust
        // to manifests produced by older scripts or third-party repackaging.
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        serde_json::from_str(text)
            .with_context(|| format!("could not parse the runtime manifest {}", path.display()))
    }

    /// Hashes every listed file. Deliberately synchronous and streaming: the base image is
    /// hundreds of megabytes and must not be read into memory.
    pub fn verify(&self, runtime_root: &Path) -> VerificationReport {
        let mut results = Vec::with_capacity(self.files.len());
        for entry in &self.files {
            let path = runtime_root.join(&entry.path);
            results.push((entry.path.clone(), verify_one(entry, &path)));
        }
        VerificationReport { results }
    }

    /// A cheap check for every startup: presence and size only. Hashing the base image on
    /// every launch would blow the cold-start budget, so the full verify is reserved for
    /// first run, after an upgrade, and on explicit request.
    pub fn verify_quick(&self, runtime_root: &Path) -> VerificationReport {
        let mut results = Vec::with_capacity(self.files.len());
        for entry in &self.files {
            let path = runtime_root.join(&entry.path);
            let verdict = match std::fs::metadata(&path) {
                Err(_) if entry.optional => FileVerdict::Ok,
                Err(_) => FileVerdict::Missing,
                Ok(metadata) if metadata.len() != entry.size_bytes => FileVerdict::WrongSize {
                    expected: entry.size_bytes,
                    actual: metadata.len(),
                },
                Ok(_) => FileVerdict::Ok,
            };
            results.push((entry.path.clone(), verdict));
        }
        VerificationReport { results }
    }
}

fn verify_one(entry: &ManifestEntry, path: &Path) -> FileVerdict {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(_) if entry.optional => return FileVerdict::Ok,
        Err(_) => return FileVerdict::Missing,
    };
    if metadata.len() != entry.size_bytes {
        return FileVerdict::WrongSize {
            expected: entry.size_bytes,
            actual: metadata.len(),
        };
    }
    match sha256_file(path) {
        Ok(actual) if actual.eq_ignore_ascii_case(&entry.sha256) => FileVerdict::Ok,
        Ok(actual) => FileVerdict::WrongHash {
            expected: entry.sha256.clone(),
            actual,
        },
        Err(err) => FileVerdict::Unreadable(err.to_string()),
    }
}

/// Streams a file through SHA-256 in 1 MiB chunks.
pub fn sha256_file(path: &Path) -> Result<String> {
    let file =
        std::fs::File::open(path).with_context(|| format!("could not open {}", path.display()))?;
    let mut reader = std::io::BufReader::with_capacity(1 << 20, file);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Builds a manifest from a directory. Used by the packaging script, not at runtime.
pub fn generate_manifest(
    runtime_root: &Path,
    runtime_version: &str,
    qemu_version: &str,
    image_version: &str,
    relative_paths: &[PathBuf],
) -> Result<Manifest> {
    let mut files = Vec::new();
    for relative in relative_paths {
        let path = runtime_root.join(relative);
        let metadata = std::fs::metadata(&path)
            .with_context(|| format!("could not stat {}", path.display()))?;
        files.push(ManifestEntry {
            path: relative.to_string_lossy().replace('\\', "/"),
            sha256: sha256_file(&path)?,
            size_bytes: metadata.len(),
            optional: false,
        });
    }
    Ok(Manifest {
        runtime_version: runtime_version.to_string(),
        qemu_version: qemu_version.to_string(),
        image_version: image_version.to_string(),
        files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest_for(dir: &Path, name: &str, content: &[u8]) -> Manifest {
        std::fs::write(dir.join(name), content).unwrap();
        Manifest {
            runtime_version: "1.0.0".into(),
            qemu_version: "9.1.0".into(),
            image_version: "debian-13.6-1".into(),
            files: vec![ManifestEntry {
                path: name.into(),
                sha256: sha256_file(&dir.join(name)).unwrap(),
                size_bytes: content.len() as u64,
                optional: false,
            }],
        }
    }

    #[test]
    fn intact_files_verify() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = manifest_for(dir.path(), "vmlinuz", b"kernel bytes");
        let report = manifest.verify(dir.path());
        assert!(report.is_healthy());
        assert!(report.problems().is_empty());
    }

    #[test]
    fn a_quarantined_file_is_reported_with_the_antivirus_hint() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = manifest_for(dir.path(), "vmlinuz", b"kernel bytes");
        std::fs::remove_file(dir.path().join("vmlinuz")).unwrap();

        let report = manifest.verify(dir.path());
        assert!(!report.is_healthy());
        let problem = &report.problems()[0];
        assert!(problem.contains("Antivirus"), "{problem}");
        assert!(problem.contains("Reinstall Runtime"), "{problem}");
    }

    #[test]
    fn a_truncated_file_is_detected_by_size_before_hashing() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = manifest_for(dir.path(), "debian-base.raw", b"0123456789");
        std::fs::write(dir.path().join("debian-base.raw"), b"01234").unwrap();

        let verdict = &manifest.verify(dir.path()).results[0].1;
        assert_eq!(
            verdict,
            &FileVerdict::WrongSize {
                expected: 10,
                actual: 5
            }
        );
    }

    #[test]
    fn a_modified_file_of_the_same_length_is_caught_by_the_hash() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = manifest_for(dir.path(), "vmlinuz", b"aaaa");
        std::fs::write(dir.path().join("vmlinuz"), b"bbbb").unwrap();

        let verdict = &manifest.verify(dir.path()).results[0].1;
        assert!(matches!(verdict, FileVerdict::WrongHash { .. }));
        // The quick check cannot see this, which is exactly why the full check exists.
        assert!(manifest.verify_quick(dir.path()).is_healthy());
    }

    #[test]
    fn optional_files_may_be_absent() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = Manifest {
            runtime_version: "1.0.0".into(),
            qemu_version: "9.1.0".into(),
            image_version: "debian-13.6-1".into(),
            files: vec![ManifestEntry {
                path: "initrd.img".into(),
                sha256: "0".repeat(64),
                size_bytes: 10,
                optional: true,
            }],
        };
        assert!(manifest.verify(dir.path()).is_healthy());
        assert!(manifest.verify_quick(dir.path()).is_healthy());
    }

    #[test]
    fn hashing_is_stable_across_chunk_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        // Larger than the 1 MiB read buffer, to exercise the streaming loop.
        let content = vec![7u8; (1 << 20) + 12345];
        let path = dir.path().join("big.raw");
        std::fs::write(&path, &content).unwrap();

        let streamed = sha256_file(&path).unwrap();
        let direct = hex::encode(Sha256::digest(&content));
        assert_eq!(streamed, direct);
    }

    #[test]
    fn generated_manifests_verify_against_their_own_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.bin"), b"one").unwrap();
        std::fs::write(dir.path().join("b.bin"), b"two").unwrap();

        let manifest = generate_manifest(
            dir.path(),
            "1.0.0",
            "9.1.0",
            "debian-13.6-1",
            &[PathBuf::from("a.bin"), PathBuf::from("b.bin")],
        )
        .unwrap();

        assert_eq!(manifest.files.len(), 2);
        assert!(manifest.verify(dir.path()).is_healthy());
    }

    #[test]
    fn manifests_with_a_utf8_bom_are_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("checksums.json");
        let manifest = Manifest {
            runtime_version: "1.0.0".into(),
            qemu_version: "9.2.0".into(),
            image_version: "test-image".into(),
            files: Vec::new(),
        };
        let json = serde_json::to_string(&manifest).unwrap();
        std::fs::write(&path, format!("\u{feff}{json}")).unwrap();

        assert_eq!(Manifest::load(&path).unwrap().runtime_version, "1.0.0");
    }

    #[test]
    fn manifest_paths_use_forward_slashes_so_they_are_portable() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("licences")).unwrap();
        std::fs::write(dir.path().join("licences/qemu.txt"), b"gpl").unwrap();

        let manifest = generate_manifest(
            dir.path(),
            "1.0.0",
            "9.1.0",
            "debian-13.6-1",
            &[PathBuf::from("licences").join("qemu.txt")],
        )
        .unwrap();
        assert_eq!(manifest.files[0].path, "licences/qemu.txt");
    }
}
