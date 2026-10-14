//! Owned process-tree termination without an unbounded Windows WMI/taskkill wait.
//!
//! A shell command is a tree (shell -> build tool -> compilers), and the tree
//! has to die as one: on timeout, on cancel, and when nur itself goes away.
//!
//! * Windows: the child is created suspended, assigned to a Job Object with
//!   `KILL_ON_JOB_CLOSE`, then resumed, so a grandchild cannot be spawned
//!   before the job exists and nothing survives nur's exit (the job handle
//!   closes with the process). The job handle lives as long as the command is
//!   supervised, including inside a `bg` job.
//! * Unix: the child leads its own process group; `terminate` kills the group,
//!   so build servers and test runners started by the shell go with it.
use std::process::{Child, Command};

/// Windows `CREATE_NO_WINDOW`: keep a console child off nur's own console.
/// Ignored on Unix so callers can pass it unconditionally.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct ProcessTree {
    #[cfg(windows)]
    job: Option<std::os::windows::io::OwnedHandle>,
    #[cfg(unix)]
    group: Option<i32>,
}

impl ProcessTree {
    /// Spawn `cmd` so that every descendant is in the tree from its first
    /// instruction.
    pub fn spawn(cmd: &mut Command) -> std::io::Result<(Child, Self)> {
        Self::spawn_with_flags(cmd, 0)
    }

    /// Like [`ProcessTree::spawn`] with extra Windows creation flags.
    /// `Command::creation_flags` replaces rather than adds, so callers that
    /// need `CREATE_NO_WINDOW` pass it here instead of setting it themselves.
    pub fn spawn_with_flags(cmd: &mut Command, flags: u32) -> std::io::Result<(Child, Self)> {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            use windows_sys::Win32::System::Threading::CREATE_SUSPENDED;
            cmd.creation_flags(flags | CREATE_SUSPENDED);
            let child = cmd.spawn()?;
            let tree = Self::attach(&child);
            // Resume whether or not the job took the process: a suspended
            // child that never runs is worse than an unjoined one.
            win::resume_primary_thread(child.id());
            Ok((child, tree))
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            let _ = flags;
            cmd.process_group(0);
            let child = cmd.spawn()?;
            Ok((
                child,
                Self {
                    group: Some(child.id() as i32),
                },
            ))
        }
    }

    /// Track an already-running child. Grandchildren spawned before this call
    /// are not in the tree; prefer [`ProcessTree::spawn`].
    pub fn attach(child: &Child) -> Self {
        #[cfg(windows)]
        {
            Self {
                job: win::job_for(child),
            }
        }
        #[cfg(unix)]
        {
            let _ = child;
            Self { group: None }
        }
    }

    pub fn terminate(&self, child: &mut Child) {
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::System::JobObjects::TerminateJobObject;
            let terminated = self
                .job
                .as_ref()
                .is_some_and(|job| unsafe { TerminateJobObject(job.as_raw_handle(), 1) != 0 });
            if !terminated {
                // Restricted parent jobs can reject assignment. Retain best
                // effort tree cleanup, but never wait indefinitely on taskkill.
                use std::process::Stdio;
                use std::time::{Duration, Instant};
                if let Ok(mut killer) = Command::new("taskkill")
                    .args(["/PID", &child.id().to_string(), "/T", "/F"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                {
                    let deadline = Instant::now() + Duration::from_millis(500);
                    while matches!(killer.try_wait(), Ok(None)) && Instant::now() < deadline {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    let _ = killer.kill();
                    let _ = killer.wait();
                }
            }
        }
        #[cfg(unix)]
        if let Some(pgid) = self.group {
            // The whole group, not just the shell: orphaned grandchildren
            // kept holding ports, files and CPU after a timeout.
            unsafe {
                libc::killpg(pgid, libc::SIGKILL);
            }
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

#[cfg(windows)]
mod win {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::process::Child;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

    /// A kill-on-close job with `child` in it, or `None` when the process
    /// cannot be assigned (a restricted parent job): callers then fall back
    /// to taskkill at termination time.
    pub(super) fn job_for(child: &Child) -> Option<OwnedHandle> {
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return None;
        }
        let owned = unsafe { OwnedHandle::from_raw_handle(job) };
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let set = unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if set == 0 {
            return None;
        }
        (unsafe { AssignProcessToJobObject(job, child.as_raw_handle()) } != 0).then_some(owned)
    }

    /// `std::process::Command` closes the primary thread handle it got from
    /// CreateProcess, so a `CREATE_SUSPENDED` child has to be found again
    /// through a thread snapshot. A suspended process has exactly one thread.
    pub(super) fn resume_primary_thread(pid: u32) {
        let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        if snap.is_null() || snap as isize == -1 {
            return;
        }
        let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        let mut ok = unsafe { Thread32First(snap, &mut entry) } != 0;
        while ok {
            if entry.th32OwnerProcessID == pid {
                let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
                if !thread.is_null() {
                    // Returns the previous suspend count; loop in case the
                    // count is above one (it is one for CREATE_SUSPENDED).
                    while unsafe { ResumeThread(thread) } > 1 {}
                    unsafe { CloseHandle(thread) };
                }
            }
            ok = unsafe { Thread32Next(snap, &mut entry) } != 0;
        }
        unsafe { CloseHandle(snap) };
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use std::process::Stdio;
    use windows_sys::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::WaitForSingleObject;

    #[test]
    #[ignore = "subprocess fixture launched by process-tree tests"]
    fn child_worker() {
        if std::env::var_os("NUR_PROCESS_TREE_TEST_CHILD").is_some() {
            std::thread::sleep(std::time::Duration::from_secs(30));
        }
    }

    fn child_cmd() -> Command {
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", "process_tree::tests::child_worker", "--ignored"])
            .env("NUR_PROCESS_TREE_TEST_CHILD", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        cmd
    }

    /// Failure modes: the suspended child is never resumed (hangs forever),
    /// or the job is dropped without killing it (survives nur).
    #[test]
    fn spawned_child_runs_and_dies_with_its_job() {
        let (mut child, tree) = ProcessTree::spawn(&mut child_cmd()).unwrap();
        let assigned = tree.job.is_some();
        // Resumed: still running after a moment, not exited and not stuck in
        // a state where it would have exited at once.
        let state = unsafe { WaitForSingleObject(child.as_raw_handle(), 200) };
        assert_eq!(state, WAIT_TIMEOUT);
        drop(tree);
        let state = unsafe { WaitForSingleObject(child.as_raw_handle(), 2_000) };
        let _ = child.kill();
        let _ = child.wait();
        assert!(assigned);
        assert_eq!(state, WAIT_OBJECT_0, "closing the job must end the tree");
    }

    #[test]
    fn native_termination_stops_the_owned_process() {
        let (mut child, tree) = ProcessTree::spawn(&mut child_cmd()).unwrap();
        let assigned = tree.job.is_some();
        let started = std::time::Instant::now();
        tree.terminate(&mut child);
        assert!(assigned);
        assert!(child.try_wait().unwrap().is_some());
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }
}
