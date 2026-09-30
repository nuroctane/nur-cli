//! Owned process-tree termination without an unbounded Windows WMI/taskkill wait.
use std::process::Child;

pub struct ProcessTree {
    #[cfg(windows)]
    job: Option<std::os::windows::io::OwnedHandle>,
}

impl ProcessTree {
    pub fn attach(child: &Child) -> Self {
        #[cfg(windows)]
        {
            use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
            use windows_sys::Win32::System::JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW,
            };
            // No KILL_ON_JOB_CLOSE: successfully launched background work must
            // survive normal completion and dropping the tracking handle.
            let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            let job = if job.is_null() {
                None
            } else {
                let owned = unsafe { OwnedHandle::from_raw_handle(job) };
                if unsafe { AssignProcessToJobObject(job, child.as_raw_handle()) } != 0 {
                    Some(owned)
                } else {
                    None
                }
            };
            Self { job }
        }
        #[cfg(not(windows))]
        {
            let _ = child;
            Self {}
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
                use std::process::{Command, Stdio};
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
        let _ = child.kill();
        let _ = child.wait();
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use std::process::{Command, Stdio};
    use windows_sys::Win32::Foundation::WAIT_TIMEOUT;
    use windows_sys::Win32::System::Threading::WaitForSingleObject;

    #[test]
    #[ignore = "subprocess fixture launched by process-tree tests"]
    fn child_worker() {
        if std::env::var_os("NUR_PROCESS_TREE_TEST_CHILD").is_some() {
            std::thread::sleep(std::time::Duration::from_secs(30));
        }
    }

    fn child() -> Child {
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "process_tree::tests::child_worker", "--ignored"])
            .env("NUR_PROCESS_TREE_TEST_CHILD", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }

    // Failure modes: dropping tracking kills valid background work, or native
    // termination fails to stop the owned process without the taskkill helper.
    #[test]
    fn releasing_tracking_preserves_successful_background_work() {
        let mut child = child();
        let tree = ProcessTree::attach(&child);
        let assigned = tree.job.is_some();
        drop(tree);
        let state = unsafe { WaitForSingleObject(child.as_raw_handle(), 100) };
        let _ = child.kill();
        let _ = child.wait();
        assert!(assigned);
        assert_eq!(state, WAIT_TIMEOUT);
    }

    #[test]
    fn native_termination_stops_the_owned_process() {
        let mut child = child();
        let tree = ProcessTree::attach(&child);
        let assigned = tree.job.is_some();
        let started = std::time::Instant::now();
        tree.terminate(&mut child);
        assert!(assigned);
        assert!(child.try_wait().unwrap().is_some());
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }
}
