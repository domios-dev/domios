#[cfg(windows)]
pub(super) use codeterm_util::windows_job::ProbeChild;

#[cfg(unix)]
pub(super) struct ProbeChild {
    child: std::process::Child,
    stdout: Option<std::process::ChildStdout>,
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

    pub(super) fn collect_output(
        &mut self,
        deadline: std::time::Instant,
        max_bytes: usize,
    ) -> std::io::Result<Option<(std::process::ExitStatus, Vec<u8>)>> {
        use std::io::Read;
        use std::os::unix::io::AsRawFd;
        use wait_timeout::ChildExt;

        let mut stdout = self.stdout.take();
        if let Some(ref pipe) = stdout {
            if let Err(error) = set_nonblocking(pipe.as_raw_fd()) {
                self.terminate();
                return Err(error);
            }
        }
        let limit = max_bytes.saturating_add(1);
        let mut output = Vec::new();
        let mut exit_status: Option<std::process::ExitStatus> = None;
        let mut buf = [0u8; 4096];

        loop {
            if let Some(pipe) = stdout.as_mut() {
                loop {
                    let want = (limit - output.len()).min(buf.len());
                    if want == 0 {
                        stdout = None;
                        break;
                    }
                    match pipe.read(&mut buf[..want]) {
                        Ok(0) => {
                            stdout = None;
                            break;
                        }
                        Ok(read) => {
                            output.extend_from_slice(&buf[..read]);
                            if output.len() >= limit {
                                stdout = None;
                                break;
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(error) => {
                            self.terminate();
                            return Err(error);
                        }
                    }
                }
            }
            if exit_status.is_none() {
                match self.child.try_wait() {
                    Ok(Some(status)) => exit_status = Some(status),
                    Ok(None) => {}
                    Err(error) => {
                        self.terminate();
                        return Err(error);
                    }
                }
            }
            if let (Some(status), None) = (exit_status, &stdout) {
                return Ok(Some((status, output)));
            }
            let now = std::time::Instant::now();
            if now >= deadline {
                self.terminate();
                return Ok(None);
            }
            let remaining = deadline.saturating_duration_since(now);
            if let Some(ref pipe) = stdout {
                let slice = if exit_status.is_none() {
                    remaining.min(std::time::Duration::from_millis(25))
                } else {
                    remaining
                };
                let timeout_ms =
                    slice.as_millis().min(u128::from(libc::c_int::MAX as u32)) as libc::c_int;
                let mut pollfd = libc::pollfd {
                    fd: pipe.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                let ready = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
                if ready < 0 {
                    let error = std::io::Error::last_os_error();
                    if error.kind() != std::io::ErrorKind::Interrupted {
                        self.terminate();
                        return Err(error);
                    }
                }
            } else {
                match self.child.wait_timeout(remaining) {
                    Ok(Some(status)) => return Ok(Some((status, output))),
                    Ok(None) => {
                        self.terminate();
                        return Ok(None);
                    }
                    Err(error) => {
                        self.terminate();
                        return Err(error);
                    }
                }
            }
        }
    }

    fn terminate(&mut self) {
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        let _ = self.child.wait();
    }
}

#[cfg(unix)]
fn set_nonblocking(fd: libc::c_int) -> std::io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(unix)]
impl Drop for ProbeChild {
    fn drop(&mut self) {
        self.terminate();
    }
}
