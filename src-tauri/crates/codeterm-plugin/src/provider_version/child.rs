#[cfg(windows)]
pub(super) use codeterm_util::windows_job::ProbeChild;

#[cfg(unix)]
pub(super) struct ProbeChild {
    child: std::process::Child,
    pub(super) stdout: Option<std::process::ChildStdout>,
}

#[cfg(unix)]
impl ProbeChild {
    pub(super) fn spawn(command: &mut std::process::Command) -> std::io::Result<Self> {
        use std::os::unix::process::CommandExt;
        use std::process::Stdio;
        command.process_group(0);
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(Self {
            stdout: child.stdout.take(),
            child,
        })
    }

    pub(super) fn wait_timeout(
        &mut self,
        timeout: std::time::Duration,
    ) -> std::io::Result<Option<std::process::ExitStatus>> {
        use wait_timeout::ChildExt;
        self.child.wait_timeout(timeout)
    }

    pub(super) fn terminate(&mut self) {
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        let _ = self.child.wait();
    }
}

#[cfg(unix)]
impl Drop for ProbeChild {
    fn drop(&mut self) {
        self.terminate();
    }
}
