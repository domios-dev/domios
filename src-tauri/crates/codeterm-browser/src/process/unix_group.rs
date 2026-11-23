use std::future::Future;
use std::io;
use std::process::{ExitStatus, Stdio};

pub(super) struct OwnedProcessGroup {
    pid: Option<libc::pid_t>,
    daemon_lifetime: Option<tokio::process::ChildStdin>,
}

impl OwnedProcessGroup {
    pub(super) fn new(pid: u32) -> io::Result<Self> {
        let pid = pid as libc::pid_t;
        let mut group = Self {
            pid: Some(pid),
            daemon_lifetime: None,
        };
        // Only a process inside the group can signal it after the daemon's pipe closes.
        let mut guardian = tokio::process::Command::new("/bin/sh")
            .args(["-c", "while IFS= read -r _; do :; done; kill -KILL 0"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(pid)
            .spawn()?;
        group.daemon_lifetime = guardian.stdin.take();
        tokio::spawn(async move {
            let _ = guardian.wait().await;
        });
        Ok(group)
    }

    pub(super) async fn reap(
        mut self,
        child: &mut tokio::process::Child,
        kill_requested: impl Future<Output = ()>,
    ) -> io::Result<ExitStatus> {
        let pid = self
            .pid
            .ok_or_else(|| io::Error::other("Missing Chromium process group"))?;
        let observed = tokio::task::spawn_blocking(move || wait_without_reaping(pid));
        tokio::select! {
            result = observed => result.map_err(io::Error::other)??,
            () = kill_requested => {}
        }
        self.terminate()?;
        child.wait().await
    }

    fn terminate(&mut self) -> io::Result<()> {
        let Some(pid) = self.pid else {
            return Ok(());
        };
        if unsafe { libc::kill(-pid, libc::SIGKILL) } != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error);
            }
        }
        self.pid = None;
        self.daemon_lifetime.take();
        Ok(())
    }
}

impl Drop for OwnedProcessGroup {
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}

fn wait_without_reaping(pid: libc::pid_t) -> io::Result<()> {
    // Keep the child's PID pinned until its group receives SIGKILL.
    let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::uninit();
    loop {
        if unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                info.as_mut_ptr(),
                libc::WEXITED | libc::WNOWAIT,
            )
        } == 0
        {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}
