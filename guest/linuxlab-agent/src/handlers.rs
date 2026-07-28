//! Request handlers for everything other than validation.

use crate::sys;
use crate::validators::{self, Ctx};
use anyhow::Result;
use shared_types::protocol::{DirEntryInfo, GuestDiagnostics, PackageInfo};
use shared_types::{AgentRequest, AgentResponse};
use std::path::{Path, PathBuf};

pub const AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Roots the host may browse for the File Tree panel. Anything outside these is refused, so a
/// compromised or buggy host cannot use the agent to read arbitrary guest paths — and more
/// importantly, cannot be used to walk out of the lesson sandbox.
const BROWSABLE_ROOTS: &[&str] = &[
    "/home",
    "/tmp",
    "/var/log",
    "/etc",
    "/opt/linuxlab/lessons",
    "/srv",
];

pub async fn handle(request: AgentRequest) -> AgentResponse {
    match request {
        AgentRequest::Ping => AgentResponse::Pong {
            agent_version: AGENT_VERSION.to_string(),
            kernel: sys::kernel_release(),
            image_version: sys::image_version(),
            uptime_seconds: sys::uptime_seconds(),
        },

        AgentRequest::PrepareLesson {
            lesson_id,
            setup_script,
            fixtures,
            namespaces,
            sudo_allowed,
        } => match prepare_lesson(
            &lesson_id,
            setup_script,
            &fixtures,
            &namespaces,
            sudo_allowed,
        )
        .await
        {
            Ok(warnings) => AgentResponse::LessonPrepared {
                lesson_id,
                warnings,
            },
            Err(err) => AgentResponse::Error {
                message: format!("could not prepare {lesson_id}: {err:#}"),
                retriable: true,
            },
        },

        AgentRequest::ValidateTask {
            lesson_id,
            task_id,
            validators: requested,
            subject_user,
            attempt_started_at,
        } => {
            let mut ctx = Ctx::new(subject_user, &lesson_id);
            ctx.attempt_started_at = attempt_started_at;
            ctx.default_namespace = active_namespace(&lesson_id);
            let validation =
                validators::validate_task(&ctx, &lesson_id, &task_id, &requested).await;
            AgentResponse::TaskValidated(validation)
        }

        AgentRequest::ResetLesson {
            lesson_id,
            reset_script,
        } => match reset_lesson(&lesson_id, reset_script).await {
            Ok(()) => AgentResponse::LessonReset { lesson_id },
            Err(err) => AgentResponse::Error {
                message: format!("could not reset {lesson_id}: {err:#}"),
                retriable: true,
            },
        },

        AgentRequest::Checkpoint { name } => match checkpoint(&name).await {
            Ok(()) => AgentResponse::CheckpointCreated { name },
            Err(err) => AgentResponse::Error {
                message: format!("could not record checkpoint {name}: {err:#}"),
                retriable: false,
            },
        },

        AgentRequest::Diagnostics => AgentResponse::Diagnostics(diagnostics().await),

        AgentRequest::ListDirectory {
            path,
            include_hidden,
        } => match list_directory(&path, include_hidden) {
            Ok(entries) => AgentResponse::DirectoryListing { path, entries },
            Err(err) => AgentResponse::Error {
                message: err.to_string(),
                retriable: false,
            },
        },

        AgentRequest::Versions => AgentResponse::Versions {
            image_version: sys::image_version(),
            agent_version: AGENT_VERSION.to_string(),
            packages: installed_packages().await,
        },

        AgentRequest::SetTerminalSize { rows, cols } => {
            // Clamped because a nonsense size from a resize race can make curses programs
            // misbehave in ways that look like a broken terminal.
            let rows = rows.clamp(10, 300);
            let cols = cols.clamp(20, 500);
            match sys::run(
                "stty",
                &[
                    "-F",
                    "/dev/ttyS0",
                    "rows",
                    &rows.to_string(),
                    "cols",
                    &cols.to_string(),
                ],
                None,
            )
            .await
            {
                Ok(output) if output.success() => AgentResponse::TerminalResized { rows, cols },
                Ok(output) => AgentResponse::Error {
                    message: format!("could not set the terminal size: {}", output.stderr.trim()),
                    retriable: true,
                },
                Err(err) => AgentResponse::Error {
                    message: err.to_string(),
                    retriable: true,
                },
            }
        }

        AgentRequest::Shutdown => {
            // Reply first, then shut down, so the host sees an orderly close rather than a
            // dropped connection it has to interpret.
            tokio::spawn(async {
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                sys::run("systemctl", &["poweroff"], None).await.ok();
            });
            AgentResponse::ShuttingDown
        }
    }
}

fn lesson_dir(lesson_id: &str) -> PathBuf {
    Ctx::new("root", lesson_id).lesson_root().to_path_buf()
}

/// Records which namespace set is active, so network validators default correctly.
fn active_namespace(lesson_id: &str) -> Option<String> {
    let marker = lesson_dir(lesson_id).join("default-namespace");
    std::fs::read_to_string(marker)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

async fn prepare_lesson(
    lesson_id: &str,
    setup_script: Option<String>,
    fixtures: &[String],
    namespaces: &[String],
    sudo_allowed: bool,
) -> Result<Vec<String>> {
    let mut warnings = Vec::new();
    let root = lesson_dir(lesson_id);
    std::fs::create_dir_all(&root)?;

    // Fixtures must not be readable by the learner, or the hidden test data stops being
    // hidden and every pipeline lesson becomes a copy-the-answer exercise.
    let fixture_root = root.join("fixtures");
    if fixture_root.exists() {
        harden_fixtures(&fixture_root)?;
    }
    for fixture in fixtures {
        if !fixture_root.join(fixture).is_dir() {
            warnings.push(format!("fixture '{fixture}' is not installed in the image"));
        }
    }

    // Controlled sudo: the lesson decides whether the student may escalate at all.
    apply_sudo_policy(sudo_allowed)?;

    if !namespaces.is_empty() {
        let list = namespaces.join(" ");
        let output = sys::run(
            "/opt/linuxlab/bin/lab-net",
            &["up", &list],
            Some(std::time::Duration::from_secs(60)),
        )
        .await?;
        if !output.success() {
            warnings.push(format!(
                "the internal network laboratory did not start cleanly: {}",
                output.stderr.trim()
            ));
        }
    }

    if let Some(script) = setup_script {
        let path = resolve_lesson_script(lesson_id, &script)?;
        let output = sys::run(
            "/bin/bash",
            &["--", &path.to_string_lossy()],
            Some(std::time::Duration::from_secs(120)),
        )
        .await?;
        if !output.success() {
            anyhow::bail!(
                "the lesson setup script failed ({}): {}",
                output.status,
                output.stderr.trim()
            );
        }
    }

    Ok(warnings)
}

/// Confines a lesson script path to that lesson's own directory.
fn resolve_lesson_script(lesson_id: &str, script: &str) -> Result<PathBuf> {
    let root = lesson_dir(lesson_id);
    let name = Path::new(script)
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("'{script}' is not a script file name"))?;
    let path = root.join(name);
    if !path.is_file() {
        anyhow::bail!("the lesson script {} is not installed", path.display());
    }
    Ok(path)
}

fn harden_fixtures(fixture_root: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fn walk(path: &Path) -> Result<()> {
        let metadata = std::fs::symlink_metadata(path)?;
        if metadata.is_dir() {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
            for entry in std::fs::read_dir(path)?.flatten() {
                walk(&entry.path())?;
            }
        } else if metadata.is_file() {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }
    walk(fixture_root)
}

/// Writes the sudoers drop-in that enables or disables escalation for the student.
///
/// The file is written atomically and validated with `visudo -c` before being put in place: a
/// malformed sudoers file locks everyone out of sudo, including the recovery path.
fn apply_sudo_policy(sudo_allowed: bool) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let target = Path::new("/etc/sudoers.d/010-linuxlab-lesson");
    let staging = Path::new("/etc/sudoers.d/.010-linuxlab-lesson.staged");

    let content = if sudo_allowed {
        "# Managed by linuxlab-agent. Lesson permits escalation.\nstudent ALL=(ALL:ALL) ALL\n"
    } else {
        "# Managed by linuxlab-agent. Lesson does not permit escalation.\n\
         student ALL=(ALL:ALL) !ALL\n"
    };

    std::fs::write(staging, content)?;
    std::fs::set_permissions(staging, std::fs::Permissions::from_mode(0o440))?;

    let check = std::process::Command::new("visudo")
        .args(["-c", "-f"])
        .arg(staging)
        .output();
    match check {
        Ok(output) if output.status.success() => {
            std::fs::rename(staging, target)?;
            Ok(())
        }
        Ok(output) => {
            std::fs::remove_file(staging).ok();
            anyhow::bail!(
                "refusing to install an invalid sudoers policy: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )
        }
        // No visudo in a stripped image: install anyway, since the content is generated here
        // rather than authored by a lesson.
        Err(_) => {
            std::fs::rename(staging, target)?;
            Ok(())
        }
    }
}

async fn reset_lesson(lesson_id: &str, reset_script: Option<String>) -> Result<()> {
    if let Some(script) = reset_script {
        let path = resolve_lesson_script(lesson_id, &script)?;
        let output = sys::run(
            "/bin/bash",
            &["--", &path.to_string_lossy()],
            Some(std::time::Duration::from_secs(120)),
        )
        .await?;
        if !output.success() {
            anyhow::bail!(
                "the lesson reset script failed ({}): {}",
                output.status,
                output.stderr.trim()
            );
        }
    }
    // Signal logs are per-attempt state and must not leak into the next one.
    if let Ok(entries) = std::fs::read_dir("/run/linuxlab/signals") {
        for entry in entries.flatten() {
            std::fs::remove_file(entry.path()).ok();
        }
    }
    Ok(())
}

async fn checkpoint(name: &str) -> Result<()> {
    let safe: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect();
    if safe.is_empty() {
        anyhow::bail!("checkpoint names must contain at least one letter or digit");
    }
    let dir = PathBuf::from("/run/linuxlab/checkpoints");
    std::fs::create_dir_all(&dir)?;
    // The durable part of a checkpoint is the host's qcow2 snapshot; this records the guest
    // side so diagnostics can line the two up.
    std::fs::write(
        dir.join(&safe),
        format!("uptime={}\n", sys::uptime_seconds()),
    )?;
    Ok(())
}

async fn diagnostics() -> GuestDiagnostics {
    let (memory_total_kb, memory_available_kb) = sys::meminfo();
    let failed_units = failed_units().await;
    let mut listening_ports: Vec<u16> = sys::read_sockets("tcp")
        .into_iter()
        .filter(|s| s.listening)
        .map(|s| s.port)
        .collect();
    listening_ports.sort_unstable();
    listening_ports.dedup();

    let (root_disk_used_percent, root_inodes_used_percent) = root_usage().await;

    GuestDiagnostics {
        hostname: sys::hostname(),
        kernel: sys::kernel_release(),
        uptime_seconds: sys::uptime_seconds(),
        load_average: sys::load_average(),
        memory_total_kb,
        memory_available_kb,
        root_disk_used_percent,
        root_inodes_used_percent,
        failed_units,
        listening_ports,
        current_directory: sys::find_login_shell("student")
            .and_then(|shell| sys::process_cwd(shell.pid))
            .map(|path| path.display().to_string()),
    }
}

async fn failed_units() -> Vec<String> {
    let Ok(output) = sys::run(
        "systemctl",
        &[
            "list-units",
            "--state=failed",
            "--no-legend",
            "--plain",
            "--no-pager",
        ],
        None,
    )
    .await
    else {
        return Vec::new();
    };
    output
        .stdout
        .lines()
        .filter_map(|line| line.split_whitespace().next().map(|s| s.to_string()))
        .collect()
}

/// Byte and inode usage of the root filesystem. Both matter: a disk-full lesson can exhaust
/// either, and the two look very different from the learner's side.
async fn root_usage() -> (u8, u8) {
    let percent_of = |output: &sys::CommandOutput| -> u8 {
        output
            .stdout
            .lines()
            .nth(1)
            .and_then(|line| {
                line.split_whitespace()
                    .find(|field| field.ends_with('%'))
                    .and_then(|field| field.trim_end_matches('%').parse().ok())
            })
            .unwrap_or(0)
    };

    let bytes = sys::run("df", &["-P", "/"], None).await;
    let inodes = sys::run("df", &["-Pi", "/"], None).await;
    (
        bytes.map(|o| percent_of(&o)).unwrap_or(0),
        inodes.map(|o| percent_of(&o)).unwrap_or(0),
    )
}

fn list_directory(path: &str, include_hidden: bool) -> Result<Vec<DirEntryInfo>> {
    let requested = PathBuf::from(path);
    let canonical = requested
        .canonicalize()
        .map_err(|err| anyhow::anyhow!("{path} could not be opened: {err}"))?;

    // Refusing anything outside the browsable roots is what keeps the File Tree panel from
    // becoming a general-purpose read primitive.
    let allowed = BROWSABLE_ROOTS
        .iter()
        .any(|root| canonical.starts_with(root));
    if !allowed {
        anyhow::bail!("{path} is outside the directories this panel may browse");
    }

    let mut entries: Vec<DirEntryInfo> = std::fs::read_dir(&canonical)?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            if !include_hidden && name.starts_with('.') {
                return None;
            }
            let facts = sys::lstat(&entry.path())?;
            Some(DirEntryInfo {
                name,
                file_type: facts.kind.as_str().to_string(),
                size: facts.size,
                mode: sys::format_mode(facts.permissions),
                owner: sys::user_for_uid(facts.uid).unwrap_or_else(|| facts.uid.to_string()),
                group: sys::group_for_gid(facts.gid).unwrap_or_else(|| facts.gid.to_string()),
                link_target: if facts.kind == sys::FileKind::Symlink {
                    std::fs::read_link(entry.path())
                        .ok()
                        .map(|p| p.display().to_string())
                } else {
                    None
                },
            })
        })
        .collect();

    // Directories first, then alphabetical: the order a file manager uses.
    entries.sort_by(|a, b| {
        let a_dir = a.file_type == "directory";
        let b_dir = b.file_type == "directory";
        b_dir.cmp(&a_dir).then_with(|| a.name.cmp(&b.name))
    });
    Ok(entries)
}

async fn installed_packages() -> Vec<PackageInfo> {
    let Ok(output) = sys::run(
        "dpkg-query",
        &["-W", "-f=${Package}\\t${Version}\\n"],
        Some(std::time::Duration::from_secs(20)),
    )
    .await
    else {
        return Vec::new();
    };
    output
        .stdout
        .lines()
        .filter_map(|line| {
            let (name, version) = line.split_once('\t')?;
            Some(PackageInfo {
                name: name.to_string(),
                version: version.to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browsing_outside_the_allowed_roots_is_refused() {
        let err = list_directory("/", false).unwrap_err().to_string();
        assert!(err.contains("outside the directories"), "{err}");
    }

    #[test]
    fn traversal_through_an_allowed_root_is_still_refused() {
        // /home/../ resolves to /, which is not browsable.
        let err = list_directory("/home/..", false).unwrap_err().to_string();
        assert!(err.contains("outside the directories"), "{err}");
    }

    #[test]
    fn an_allowed_root_can_be_listed_and_is_ordered_directories_first() {
        let entries = list_directory("/etc", false).expect("/etc should be listable");
        assert!(!entries.is_empty());
        let first_file = entries.iter().position(|e| e.file_type != "directory");
        let last_directory = entries.iter().rposition(|e| e.file_type == "directory");
        if let (Some(first_file), Some(last_directory)) = (first_file, last_directory) {
            assert!(last_directory < first_file, "directories must sort first");
        }
    }

    #[test]
    fn hidden_entries_are_excluded_unless_requested() {
        let dir = tempfile::tempdir().unwrap();
        // Use /tmp, which is a browsable root, so the containment check passes.
        let scratch = PathBuf::from("/tmp").join(format!(
            "linuxlab-test-{}",
            dir.path().file_name().unwrap().to_string_lossy()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        std::fs::write(scratch.join(".hidden"), b"").unwrap();
        std::fs::write(scratch.join("visible"), b"").unwrap();

        let rendered = scratch.to_string_lossy().to_string();
        let without = list_directory(&rendered, false).unwrap();
        assert_eq!(without.len(), 1);
        let with = list_directory(&rendered, true).unwrap();
        assert_eq!(with.len(), 2);

        std::fs::remove_dir_all(&scratch).ok();
    }

    #[test]
    fn lesson_scripts_cannot_be_read_from_outside_the_lesson_directory() {
        let err = resolve_lesson_script("m.01", "../../../etc/shadow")
            .unwrap_err()
            .to_string();
        // Only the file name survives, so the resolved path stays inside the lesson root.
        assert!(err.contains("/opt/linuxlab/lessons/m.01/shadow"), "{err}");
    }

    #[test]
    fn checkpoint_names_must_contain_something_usable() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = runtime.block_on(checkpoint("///")).unwrap_err().to_string();
        assert!(err.contains("at least one letter"), "{err}");
    }

    #[tokio::test]
    async fn ping_reports_the_agent_version() {
        let response = handle(AgentRequest::Ping).await;
        match response {
            AgentResponse::Pong { agent_version, .. } => {
                assert_eq!(agent_version, AGENT_VERSION);
            }
            other => panic!("expected Pong, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn diagnostics_never_include_command_history_or_file_contents() {
        // Spec 5.4: diagnostics are "safe"; validation must not depend on history, and the
        // panel must not become a way to read the learner's files.
        let response = handle(AgentRequest::Diagnostics).await;
        let json = serde_json::to_string(&response).unwrap();
        for forbidden in ["bash_history", "command_history", "contents"] {
            assert!(!json.contains(forbidden), "diagnostics leaked {forbidden}");
        }
    }
}
