//! Thin helpers over the pieces of Linux the validators inspect.
//!
//! Two principles run through this module. Where a fact is available by reading `/proc` or
//! `/etc` directly, we read it, because parsing a tool's human-readable output is fragile and
//! locale-dependent. Where the fact genuinely lives behind a tool — systemd state, apt's
//! database, nftables — we shell out, but always with a timeout, because a validator that
//! hangs looks to the learner like a frozen application.

use anyhow::{anyhow, Context, Result};
#[cfg(test)]
use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

impl CommandOutput {
    pub fn success(&self) -> bool {
        self.status == 0 && !self.timed_out
    }

    pub fn stdout_trimmed(&self) -> &str {
        self.stdout.trim()
    }
}

/// Runs a command with no shell involved.
pub async fn run(program: &str, args: &[&str], timeout: Option<Duration>) -> Result<CommandOutput> {
    run_with(program, args, timeout, None, None).await
}

/// Runs a shell pipeline. Needed by the script validators, whose whole subject is pipelines.
///
/// This executes lesson-authored text as root inside a disposable guest. That is acceptable
/// precisely because the guest is disposable and isolated from Windows; it would not be
/// acceptable on the host, which is why no equivalent exists there.
pub async fn run_shell(
    command: &str,
    user: Option<&str>,
    working_dir: Option<&Path>,
    timeout: Option<Duration>,
) -> Result<CommandOutput> {
    match user {
        None | Some("root") => {
            run_with("/bin/bash", &["-lc", command], timeout, working_dir, None).await
        }
        Some(user) => {
            run_with(
                "runuser",
                &["-u", user, "--", "/bin/bash", "-lc", command],
                timeout,
                working_dir,
                None,
            )
            .await
        }
    }
}

async fn run_with(
    program: &str,
    args: &[&str],
    timeout: Option<Duration>,
    working_dir: Option<&Path>,
    stdin: Option<&str>,
) -> Result<CommandOutput> {
    use std::process::Stdio;
    use tokio::io::AsyncWriteExt;

    let mut command = Command::new(program);
    command
        .args(args)
        // A timed-out validator must not leave a child process running in the lesson VM.
        .kill_on_drop(true)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // A predictable, C-locale environment so validators never depend on the learner's
        // shell configuration or language settings.
        .env_clear()
        .env(
            "PATH",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        )
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .env("TERM", "dumb");

    if let Some(dir) = working_dir {
        command.current_dir(dir);
    }

    let mut child = command
        .spawn()
        .with_context(|| format!("could not run {program}"))?;

    if let Some(input) = stdin {
        if let Some(mut pipe) = child.stdin.take() {
            pipe.write_all(input.as_bytes()).await.ok();
            pipe.shutdown().await.ok();
        }
    }

    let timeout = timeout.unwrap_or(DEFAULT_TIMEOUT);
    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(output) => {
            let output = output.with_context(|| format!("{program} failed to run"))?;
            Ok(CommandOutput {
                status: output.status.code().unwrap_or(-1),
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                timed_out: false,
            })
        }
        Err(_) => Ok(CommandOutput {
            status: -1,
            stdout: String::new(),
            stderr: format!("timed out after {}s", timeout.as_secs()),
            timed_out: true,
        }),
    }
}

/// Runs a command with data on stdin. Used by `script_exit_code`.
pub async fn run_with_stdin(
    program: &str,
    args: &[&str],
    stdin: &str,
    timeout: Option<Duration>,
    working_dir: Option<&Path>,
) -> Result<CommandOutput> {
    run_with(program, args, timeout, working_dir, Some(stdin)).await
}

/// Runs a command inside a network namespace, for internal-lab lessons.
pub async fn run_in_namespace(
    namespace: &str,
    program: &str,
    args: &[&str],
    timeout: Option<Duration>,
) -> Result<CommandOutput> {
    let mut full: Vec<&str> = vec!["netns", "exec", namespace, program];
    full.extend_from_slice(args);
    run("ip", &full, timeout).await
}

// ---------------------------------------------------------------------------
// Users and groups
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswdEntry {
    pub name: String,
    pub uid: u32,
    pub gid: u32,
    pub home: String,
    pub shell: String,
}

/// Parses `/etc/passwd`. Reading the file directly means lessons that edit `/etc/passwd`
/// by hand — which spec 15.3 explicitly allows in Dangerous Mode — are observed accurately.
pub fn read_passwd() -> Result<Vec<PasswdEntry>> {
    parse_passwd(&std::fs::read_to_string("/etc/passwd").context("could not read /etc/passwd")?)
}

pub fn parse_passwd(content: &str) -> Result<Vec<PasswdEntry>> {
    let mut entries = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() < 7 {
            // A malformed line is what a broken-system lesson looks like. Skip it rather
            // than failing the whole validator run.
            continue;
        }
        let (Ok(uid), Ok(gid)) = (fields[2].parse(), fields[3].parse()) else {
            continue;
        };
        entries.push(PasswdEntry {
            name: fields[0].to_string(),
            uid,
            gid,
            home: fields[5].to_string(),
            shell: fields[6].to_string(),
        });
    }
    Ok(entries)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupEntry {
    pub name: String,
    pub gid: u32,
    pub members: Vec<String>,
}

pub fn read_groups() -> Result<Vec<GroupEntry>> {
    parse_groups(&std::fs::read_to_string("/etc/group").context("could not read /etc/group")?)
}

pub fn parse_groups(content: &str) -> Result<Vec<GroupEntry>> {
    let mut entries = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() < 4 {
            continue;
        }
        let Ok(gid) = fields[2].parse() else { continue };
        entries.push(GroupEntry {
            name: fields[0].to_string(),
            gid,
            members: fields[3]
                .split(',')
                .filter(|m| !m.is_empty())
                .map(|m| m.to_string())
                .collect(),
        });
    }
    Ok(entries)
}

pub fn uid_for(user: &str) -> Result<u32> {
    read_passwd()?
        .into_iter()
        .find(|e| e.name == user)
        .map(|e| e.uid)
        .ok_or_else(|| anyhow!("no such user {user}"))
}

pub fn user_for_uid(uid: u32) -> Option<String> {
    read_passwd()
        .ok()?
        .into_iter()
        .find(|e| e.uid == uid)
        .map(|e| e.name)
}

pub fn group_for_gid(gid: u32) -> Option<String> {
    read_groups()
        .ok()?
        .into_iter()
        .find(|e| e.gid == gid)
        .map(|e| e.name)
}

/// Every group a user belongs to, primary and supplementary.
pub fn groups_for_user(user: &str) -> Result<Vec<String>> {
    let passwd = read_passwd()?;
    let groups = read_groups()?;
    let primary_gid = passwd.iter().find(|e| e.name == user).map(|e| e.gid);

    let mut names: Vec<String> = groups
        .iter()
        .filter(|g| g.members.iter().any(|m| m == user) || Some(g.gid) == primary_gid)
        .map(|g| g.name.clone())
        .collect();
    names.sort();
    names.dedup();
    Ok(names)
}

// ---------------------------------------------------------------------------
// File metadata
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileKind {
    Regular,
    Directory,
    Symlink,
    Fifo,
    Socket,
    Block,
    Char,
    Unknown,
}

impl FileKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Regular => "regular",
            Self::Directory => "directory",
            Self::Symlink => "symlink",
            Self::Fifo => "fifo",
            Self::Socket => "socket",
            Self::Block => "block",
            Self::Char => "char",
            Self::Unknown => "unknown",
        }
    }

    pub fn from_mode(mode: u32) -> Self {
        match mode & 0o170000 {
            0o100000 => Self::Regular,
            0o040000 => Self::Directory,
            0o120000 => Self::Symlink,
            0o010000 => Self::Fifo,
            0o140000 => Self::Socket,
            0o060000 => Self::Block,
            0o020000 => Self::Char,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileFacts {
    pub kind: FileKind,
    /// Permission and special bits only, i.e. mode & 0o7777.
    pub permissions: u32,
    pub size: u64,
    pub uid: u32,
    pub gid: u32,
    pub inode: u64,
}

/// Facts about a path without following a final symlink, so `symbolic_link_exists` and
/// `file_type` can tell a link from its target.
pub fn lstat(path: &Path) -> Option<FileFacts> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    Some(FileFacts {
        kind: FileKind::from_mode(metadata.mode()),
        permissions: metadata.mode() & 0o7777,
        size: metadata.size(),
        uid: metadata.uid(),
        gid: metadata.gid(),
        inode: metadata.ino(),
    })
}

/// Facts about a path with symlinks followed.
pub fn stat(path: &Path) -> Option<FileFacts> {
    let metadata = std::fs::metadata(path).ok()?;
    Some(FileFacts {
        kind: FileKind::from_mode(metadata.mode()),
        permissions: metadata.mode() & 0o7777,
        size: metadata.size(),
        uid: metadata.uid(),
        gid: metadata.gid(),
        inode: metadata.ino(),
    })
}

pub fn format_mode(mode: u32) -> String {
    format!("{:04o}", mode & 0o7777)
}

/// Accepts a mode written as `"0644"`, `"644"` or the number 420, since lesson authors and
/// JSON both have opinions about leading zeros.
pub fn parse_mode(value: &serde_json::Value) -> Option<u32> {
    match value {
        serde_json::Value::String(s) => {
            u32::from_str_radix(s.trim().trim_start_matches("0o"), 8).ok()
        }
        serde_json::Value::Number(n) => n.as_u64().map(|v| v as u32),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Processes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ProcessFacts {
    pub pid: u32,
    pub ppid: u32,
    pub uid: u32,
    /// Full command line with arguments separated by single spaces.
    pub cmdline: String,
    /// The executable name from `/proc/<pid>/stat`, which survives an empty cmdline.
    pub comm: String,
    pub session_id: u32,
    /// Foreground process group of the controlling terminal, or -1.
    pub tpgid: i32,
    pub pgrp: u32,
}

impl ProcessFacts {
    /// What to match validator patterns against: the command line when there is one, and the
    /// executable name otherwise (kernel threads, and processes that scrubbed their argv).
    pub fn match_text(&self) -> &str {
        if self.cmdline.trim().is_empty() {
            &self.comm
        } else {
            &self.cmdline
        }
    }
}

/// Snapshot of every process the agent can see.
pub fn read_processes() -> Vec<ProcessFacts> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let pid: u32 = entry.file_name().to_string_lossy().parse().ok()?;
            read_process(pid)
        })
        .collect()
}

pub fn read_process(pid: u32) -> Option<ProcessFacts> {
    let root = PathBuf::from(format!("/proc/{pid}"));
    let stat_raw = std::fs::read_to_string(root.join("stat")).ok()?;
    let (comm, rest) = split_proc_stat(&stat_raw)?;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    // Field indices are relative to `rest`, which begins at the state field (stat field 3).
    fields.first()?;
    let ppid = fields.get(1)?.parse().unwrap_or(0);
    let pgrp = fields.get(2)?.parse().unwrap_or(0);
    let session_id = fields.get(3)?.parse().unwrap_or(0);
    let tpgid = fields.get(5)?.parse().unwrap_or(-1);

    let cmdline = std::fs::read(root.join("cmdline"))
        .map(|bytes| {
            bytes
                .split(|b| *b == 0)
                .filter(|part| !part.is_empty())
                .map(|part| String::from_utf8_lossy(part).to_string())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();

    let uid = std::fs::read_to_string(root.join("status"))
        .ok()
        .and_then(|status| parse_status_uid(&status))
        .unwrap_or(0);

    Some(ProcessFacts {
        pid,
        ppid,
        uid,
        cmdline,
        comm,
        session_id,
        tpgid,
        pgrp,
    })
}

/// Splits `/proc/<pid>/stat` around the comm field.
///
/// The comm field is wrapped in parentheses and may itself contain spaces and parentheses,
/// which is why splitting the whole line on whitespace is wrong. Finding the *last* `)`
/// is the standard way to do this correctly.
pub fn split_proc_stat(stat: &str) -> Option<(String, &str)> {
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    if close < open {
        return None;
    }
    let comm = stat[open + 1..close].to_string();
    Some((comm, stat[close + 1..].trim_start()))
}

pub fn parse_status_uid(status: &str) -> Option<u32> {
    status
        .lines()
        .find(|line| line.starts_with("Uid:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

/// The learner's interactive login shell.
///
/// Prefers the session leader, because that is the shell attached to the serial console
/// rather than a subshell a script happened to spawn. Falls back to the most recently
/// started matching shell.
pub fn find_login_shell(user: &str) -> Option<ProcessFacts> {
    let uid = uid_for(user).ok()?;
    let processes = read_processes();
    let shells: Vec<&ProcessFacts> = processes
        .iter()
        .filter(|p| p.uid == uid && is_shell(&p.comm))
        .collect();

    shells
        .iter()
        .find(|p| p.pid == p.session_id)
        .or_else(|| shells.iter().max_by_key(|p| p.pid))
        .map(|p| (*p).clone())
}

fn is_shell(comm: &str) -> bool {
    matches!(comm, "bash" | "sh" | "dash" | "-bash")
}

/// Working directory of a process, resolved through `/proc/<pid>/cwd`.
pub fn process_cwd(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

// ---------------------------------------------------------------------------
// Listening sockets
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocketFacts {
    pub local_address: String,
    pub port: u16,
    pub listening: bool,
    pub inode: u64,
}

/// Reads listening sockets from `/proc/net/*`.
///
/// Parsing `/proc` rather than `ss` output keeps this independent of iproute2's formatting and
/// works even in a lesson where the learner has broken their PATH.
pub fn read_sockets(protocol: &str) -> Vec<SocketFacts> {
    let mut sockets = Vec::new();
    let files: &[&str] = match protocol {
        "udp" => &["/proc/net/udp", "/proc/net/udp6"],
        _ => &["/proc/net/tcp", "/proc/net/tcp6"],
    };
    for file in files {
        let Ok(content) = std::fs::read_to_string(file) else {
            continue;
        };
        sockets.extend(parse_proc_net(&content, protocol == "udp"));
    }
    sockets
}

/// `/proc/net/tcp` uses hex, big-endian-per-word addresses and a numeric state column.
/// State `0A` is LISTEN; UDP has no listen state, so a bound socket counts.
pub fn parse_proc_net(content: &str, is_udp: bool) -> Vec<SocketFacts> {
    let mut sockets = Vec::new();
    for line in content.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 10 {
            continue;
        }
        let Some((address_hex, port_hex)) = fields[1].split_once(':') else {
            continue;
        };
        let Ok(port) = u16::from_str_radix(port_hex, 16) else {
            continue;
        };
        let listening = is_udp || fields[3].eq_ignore_ascii_case("0A");
        let inode = fields[9].parse().unwrap_or(0);
        sockets.push(SocketFacts {
            local_address: decode_hex_address(address_hex),
            port,
            listening,
            inode,
        });
    }
    sockets
}

fn decode_hex_address(hex: &str) -> String {
    match hex.len() {
        8 => {
            // IPv4, stored little-endian within one 32-bit word.
            let Ok(raw) = u32::from_str_radix(hex, 16) else {
                return hex.to_string();
            };
            let octets = raw.to_le_bytes();
            format!("{}.{}.{}.{}", octets[0], octets[1], octets[2], octets[3])
        }
        32 => {
            // IPv6, stored as four little-endian 32-bit words.
            let mut bytes = [0u8; 16];
            for word in 0..4 {
                let Ok(raw) = u32::from_str_radix(&hex[word * 8..word * 8 + 8], 16) else {
                    return hex.to_string();
                };
                bytes[word * 4..word * 4 + 4].copy_from_slice(&raw.to_le_bytes());
            }
            if bytes.iter().all(|b| *b == 0) {
                return "::".to_string();
            }
            let groups: Vec<String> = bytes
                .chunks(2)
                .map(|pair| format!("{:x}", u16::from_be_bytes([pair[0], pair[1]])))
                .collect();
            groups.join(":")
        }
        _ => hex.to_string(),
    }
}

// ---------------------------------------------------------------------------
// System facts
// ---------------------------------------------------------------------------

pub fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "linuxlab".to_string())
}

pub fn kernel_release() -> String {
    std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

pub fn uptime_seconds() -> u64 {
    std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok())
        .map(|secs| secs as u64)
        .unwrap_or(0)
}

pub fn load_average() -> [f32; 3] {
    std::fs::read_to_string("/proc/loadavg")
        .ok()
        .map(|content| {
            let parts: Vec<f32> = content
                .split_whitespace()
                .take(3)
                .filter_map(|p| p.parse().ok())
                .collect();
            [
                parts.first().copied().unwrap_or(0.0),
                parts.get(1).copied().unwrap_or(0.0),
                parts.get(2).copied().unwrap_or(0.0),
            ]
        })
        .unwrap_or([0.0; 3])
}

/// `MemTotal` and `MemAvailable` in kibibytes.
pub fn meminfo() -> (u64, u64) {
    let Ok(content) = std::fs::read_to_string("/proc/meminfo") else {
        return (0, 0);
    };
    let value_of = |key: &str| -> u64 {
        content
            .lines()
            .find(|line| line.starts_with(key))
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    };
    (value_of("MemTotal:"), value_of("MemAvailable:"))
}

/// Reads the LinuxLab image version stamped in by the image builder.
pub fn image_version() -> String {
    std::fs::read_to_string("/opt/linuxlab/image-version")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Parses `key=value` lines, as used by `/etc/os-release`.
#[cfg(test)]
pub fn parse_env_file(content: &str) -> HashMap<String, String> {
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            Some((
                key.trim().to_string(),
                value.trim().trim_matches('"').to_string(),
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwd_parsing_extracts_the_fields_validators_need() {
        let entries = parse_passwd(
            "root:x:0:0:root:/root:/bin/bash\n\
             student:x:1000:1000:Student:/home/student:/bin/bash\n\
             # a comment\n\
             broken-line\n",
        )
        .unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].name, "student");
        assert_eq!(entries[1].uid, 1000);
        assert_eq!(entries[1].home, "/home/student");
        assert_eq!(entries[1].shell, "/bin/bash");
    }

    #[test]
    fn a_mangled_passwd_line_is_skipped_not_fatal() {
        // A lesson may deliberately corrupt /etc/passwd; validators must still run.
        let entries = parse_passwd(
            "student:x:notanumber:1000:S:/home/student:/bin/bash\nroot:x:0:0:r:/root:/bin/sh",
        )
        .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "root");
    }

    #[test]
    fn group_parsing_reads_members() {
        let groups = parse_groups("developers:x:1001:student,analyst\nempty:x:1002:\n").unwrap();
        assert_eq!(groups[0].members, vec!["student", "analyst"]);
        assert!(groups[1].members.is_empty());
    }

    #[test]
    fn proc_stat_comm_containing_spaces_and_parens_is_split_correctly() {
        // This is the classic /proc parsing bug: "(my program) R 1 ..." breaks naive splitting.
        let (comm, rest) =
            split_proc_stat("42 (my (odd) program) S 1 42 42 1024 -1 4194304").unwrap();
        assert_eq!(comm, "my (odd) program");
        assert!(rest.starts_with("S 1 42 42"));
    }

    #[test]
    fn status_uid_is_the_real_uid() {
        let status = "Name:\tbash\nState:\tS (sleeping)\nUid:\t1000\t1000\t1000\t1000\n";
        assert_eq!(parse_status_uid(status), Some(1000));
    }

    #[test]
    fn file_kinds_are_decoded_from_the_mode() {
        assert_eq!(FileKind::from_mode(0o100644), FileKind::Regular);
        assert_eq!(FileKind::from_mode(0o040755), FileKind::Directory);
        assert_eq!(FileKind::from_mode(0o120777), FileKind::Symlink);
        assert_eq!(FileKind::from_mode(0o010644), FileKind::Fifo);
    }

    #[test]
    fn modes_are_formatted_with_four_octal_digits() {
        assert_eq!(format_mode(0o100644), "0644");
        assert_eq!(format_mode(0o4755), "4755");
    }

    #[test]
    fn modes_are_parsed_from_the_forms_authors_actually_write() {
        assert_eq!(parse_mode(&serde_json::json!("0644")), Some(0o644));
        assert_eq!(parse_mode(&serde_json::json!("644")), Some(0o644));
        assert_eq!(parse_mode(&serde_json::json!("0o600")), Some(0o600));
        assert_eq!(parse_mode(&serde_json::json!(420)), Some(420));
        assert_eq!(parse_mode(&serde_json::json!("rwx")), None);
    }

    #[test]
    fn listening_tcp_sockets_are_recognised_and_bound_addresses_decoded() {
        // 0100007F:1F90 is 127.0.0.1:8080; 0A is LISTEN. The second row is ESTABLISHED.
        let content = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n\
                       0: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 12345\n\
                       1: 0100007F:0016 0100007F:C001 01 00000000:00000000 00:00000000 00000000     0        0 12346\n";
        let sockets = parse_proc_net(content, false);
        assert_eq!(sockets.len(), 2);
        assert_eq!(sockets[0].port, 8080);
        assert_eq!(sockets[0].local_address, "127.0.0.1");
        assert!(sockets[0].listening);
        assert_eq!(sockets[0].inode, 12345);
        assert!(!sockets[1].listening, "ESTABLISHED is not LISTEN");
    }

    #[test]
    fn a_wildcard_bind_decodes_to_all_interfaces() {
        let content = "  sl  local_address\n0: 00000000:0050 00000000:0000 0A 0 0 0 0 0 999\n";
        let sockets = parse_proc_net(content, false);
        assert_eq!(sockets[0].local_address, "0.0.0.0");
        assert_eq!(sockets[0].port, 80);
    }

    #[test]
    fn udp_sockets_count_as_bound_without_a_listen_state() {
        let content = "  sl  local_address\n0: 00000000:0035 00000000:0000 07 0 0 0 0 0 555\n";
        let sockets = parse_proc_net(content, true);
        assert!(sockets[0].listening);
        assert_eq!(sockets[0].port, 53);
    }

    #[test]
    fn ipv6_wildcard_is_decoded() {
        let hex = "0".repeat(32);
        let content = format!("  sl  local_address\n0: {hex}:0050 00000000000000000000000000000000:0000 0A 0 0 0 0 0 777\n");
        let sockets = parse_proc_net(&content, false);
        assert_eq!(sockets[0].local_address, "::");
    }

    #[test]
    fn env_files_are_parsed_with_quotes_stripped() {
        let parsed = parse_env_file(
            "# comment\nID=debian\nPRETTY_NAME=\"Debian GNU/Linux 13 (trixie)\"\n\n",
        );
        assert_eq!(parsed["ID"], "debian");
        assert_eq!(parsed["PRETTY_NAME"], "Debian GNU/Linux 13 (trixie)");
        assert_eq!(parsed.len(), 2);
    }

    #[test]
    fn match_text_falls_back_to_comm_for_an_empty_cmdline() {
        let mut facts = ProcessFacts {
            pid: 1,
            ppid: 0,
            uid: 0,
            cmdline: String::new(),
            comm: "kthreadd".into(),
            session_id: 1,
            tpgid: -1,
            pgrp: 1,
        };
        assert_eq!(facts.match_text(), "kthreadd");
        facts.cmdline = "/usr/sbin/nginx -g daemon off;".into();
        assert_eq!(facts.match_text(), "/usr/sbin/nginx -g daemon off;");
    }
}
