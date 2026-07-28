//! Process supervision and host containment.
//!
//! Two guarantees live here, and both are acceptance criteria:
//!   * no QEMU process survives the desktop application (spec 27.12), and
//!   * a stale QEMU from a previous crash is found and killed before a new one starts
//!     (spec 19.2, 20).
//!
//! The mechanism is a Windows Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. Because
//! the job handle is owned by the application process, the kernel terminates QEMU even if we
//! are killed with no chance to run cleanup code.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::{Child, Command};

/// A running QEMU instance plus the containment that outlives our own error handling.
pub struct Supervised {
    pub child: Child,
    pub pid: u32,
    #[cfg(windows)]
    _job: windows_impl::JobHandle,
}

impl Supervised {
    /// Best-effort termination. Callers should try a QMP powerdown first.
    pub async fn terminate(&mut self) -> Result<()> {
        self.child.start_kill().ok();
        // The job object is the backstop, so a failure to reap is not fatal.
        match tokio::time::timeout(std::time::Duration::from_secs(5), self.child.wait()).await {
            Ok(Ok(status)) => tracing::info!("qemu exited with {status:?}"),
            Ok(Err(err)) => tracing::warn!("failed to reap qemu: {err:#}"),
            Err(_) => tracing::warn!("qemu did not exit within 5s; the job object will reap it"),
        }
        Ok(())
    }
}

/// Records the PID of the QEMU we started so the next launch can detect a stale one.
pub struct PidFile {
    path: PathBuf,
}

impl PidFile {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join("vm.pid"),
        }
    }

    pub fn write(&self, pid: u32) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(&self.path, pid.to_string())
            .with_context(|| format!("could not write {}", self.path.display()))
    }

    pub fn read(&self) -> Option<u32> {
        std::fs::read_to_string(&self.path)
            .ok()?
            .trim()
            .parse()
            .ok()
    }

    pub fn clear(&self) {
        std::fs::remove_file(&self.path).ok();
    }
}

/// Spawns QEMU inside a kill-on-close job object, with no inherited console and no stdio
/// handles the guest could reach.
pub async fn spawn(
    qemu_binary: &Path,
    args: &[String],
    working_dir: &Path,
    log_path: &Path,
) -> Result<Supervised> {
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let stderr_log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .with_context(|| format!("could not open {}", log_path.display()))?;

    let mut command = Command::new(qemu_binary);
    command
        .args(args)
        .current_dir(working_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr_log))
        .kill_on_drop(true);

    // Clear inherited environment except what QEMU genuinely needs. The guest cannot read
    // host environment variables anyway, but a smaller surface is a smaller surface.
    command.env_clear();
    for key in ["SystemRoot", "windir", "TEMP", "TMP", "PATH"] {
        if let Ok(value) = std::env::var(key) {
            command.env(key, value);
        }
    }

    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW: no console flashes on screen.
        // CREATE_SUSPENDED: lets us attach the job before a single guest instruction runs,
        // closing the window where a crash could orphan the process.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const CREATE_SUSPENDED: u32 = 0x0000_0004;
        command.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
    }

    let child = command
        .spawn()
        .with_context(|| format!("could not start {}", qemu_binary.display()))?;
    let pid = child.id().context("spawned process has no pid")?;

    #[cfg(windows)]
    {
        let job = windows_impl::JobHandle::create_and_assign(pid)
            .context("could not contain QEMU in a job object")?;
        windows_impl::resume_process(pid).context("could not resume QEMU after containment")?;
        Ok(Supervised {
            child,
            pid,
            _job: job,
        })
    }

    #[cfg(not(windows))]
    Ok(Supervised { child, pid })
}

/// Kills a QEMU left behind by an unclean shutdown.
///
/// The PID is only acted on when the running image actually looks like our bundled QEMU,
/// because PIDs are recycled and killing an unrelated process would be a serious bug.
pub fn kill_stale(pid: u32, expected_binary: &Path) -> Result<bool> {
    #[cfg(windows)]
    {
        let Some(image) = windows_impl::process_image_path(pid)? else {
            return Ok(false);
        };
        let matches = Path::new(&image)
            .file_name()
            .zip(expected_binary.file_name())
            .map(|(a, b)| a.eq_ignore_ascii_case(b))
            .unwrap_or(false);
        if !matches {
            tracing::info!("pid {pid} is {image}, not our QEMU; leaving it alone",);
            return Ok(false);
        }
        windows_impl::terminate(pid)?;
        tracing::info!("terminated stale QEMU pid {pid}");
        Ok(true)
    }
    #[cfg(not(windows))]
    {
        let _ = (pid, expected_binary);
        Ok(false)
    }
}

#[cfg(windows)]
mod windows_impl {
    use anyhow::{anyhow, Result};
    use std::ffi::c_void;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject, JOBOBJECTINFOCLASS,
        JOBOBJECT_BASIC_LIMIT_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, OpenThread, QueryFullProcessImageNameW, ResumeThread, TerminateProcess,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
        THREAD_SUSPEND_RESUME,
    };

    const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: JOBOBJECTINFOCLASS = 9;

    /// Owns the job object. Dropping it closes the handle, and because the job carries
    /// `KILL_ON_JOB_CLOSE`, that terminates every process still inside.
    pub struct JobHandle(HANDLE);

    // The handle is only ever closed on drop; sharing it across threads is safe.
    unsafe impl Send for JobHandle {}
    unsafe impl Sync for JobHandle {}

    impl JobHandle {
        pub fn create_and_assign(pid: u32) -> Result<Self> {
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if job.is_null() {
                    return Err(anyhow!("CreateJobObject failed"));
                }
                let job = JobHandle(job);

                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
                    BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
                        // Only KILL_ON_JOB_CLOSE. Breakaway flags are intentionally absent so
                        // QEMU cannot place a child process outside the job and survive us.
                        LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                        ..std::mem::zeroed()
                    },
                    ..std::mem::zeroed()
                };

                let ok = SetInformationJobObject(
                    job.0,
                    JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
                    &mut info as *mut _ as *mut c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if ok == 0 {
                    return Err(anyhow!("SetInformationJobObject failed"));
                }

                let process = OpenProcess(
                    PROCESS_SET_QUOTA | PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
                    0,
                    pid,
                );
                if process.is_null() {
                    return Err(anyhow!("OpenProcess failed for pid {pid}"));
                }
                let assigned = AssignProcessToJobObject(job.0, process);
                CloseHandle(process);
                if assigned == 0 {
                    return Err(anyhow!("AssignProcessToJobObject failed for pid {pid}"));
                }
                Ok(job)
            }
        }
    }

    impl Drop for JobHandle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    /// Resumes a process created with CREATE_SUSPENDED by resuming its primary thread.
    pub fn resume_process(pid: u32) -> Result<()> {
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
        };
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return Err(anyhow!("CreateToolhelp32Snapshot failed"));
            }
            let mut entry: THREADENTRY32 = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
            let mut resumed = false;
            if Thread32First(snapshot, &mut entry) != 0 {
                loop {
                    if entry.th32OwnerProcessID == pid {
                        let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                        if !thread.is_null() {
                            ResumeThread(thread);
                            CloseHandle(thread);
                            resumed = true;
                        }
                    }
                    entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
                    if Thread32Next(snapshot, &mut entry) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snapshot);
            if resumed {
                Ok(())
            } else {
                Err(anyhow!("found no thread to resume for pid {pid}"))
            }
        }
    }

    pub fn process_image_path(pid: u32) -> Result<Option<String>> {
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                // Gone already, or not ours to inspect. Either way, nothing to kill.
                return Ok(None);
            }
            let mut buffer = [0u16; 32768];
            let mut size = buffer.len() as u32;
            let ok = QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut size);
            CloseHandle(process);
            if ok == 0 {
                return Ok(None);
            }
            Ok(Some(String::from_utf16_lossy(&buffer[..size as usize])))
        }
    }

    pub fn terminate(pid: u32) -> Result<()> {
        unsafe {
            let process = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if process.is_null() {
                return Ok(());
            }
            let ok = TerminateProcess(process, 1);
            CloseHandle(process);
            if ok == 0 {
                return Err(anyhow!("TerminateProcess failed for pid {pid}"));
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pid_file_roundtrips_and_clears() {
        let dir = tempfile::tempdir().unwrap();
        let file = PidFile::new(dir.path());
        assert_eq!(file.read(), None);
        file.write(4242).unwrap();
        assert_eq!(file.read(), Some(4242));
        file.clear();
        assert_eq!(file.read(), None);
    }

    #[test]
    fn garbage_pid_file_is_ignored_rather_than_panicking() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("vm.pid"), "not-a-pid").unwrap();
        assert_eq!(PidFile::new(dir.path()).read(), None);
    }

    #[test]
    fn stale_cleanup_refuses_to_kill_an_unrelated_process() {
        // Our own process is certainly not qemu-system-x86_64.
        let me = std::process::id();
        let killed = kill_stale(me, Path::new("qemu-system-x86_64.exe")).unwrap();
        assert!(
            !killed,
            "must never kill a process whose image does not match"
        );
    }
}
