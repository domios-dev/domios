use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use wait_timeout::ChildExt;

pub const PROBE_RUNTIME: Duration = Duration::from_secs(3);
pub const MAX_OUTPUT_BYTES: usize = 65_536;

#[derive(Debug)]
pub struct Output {
    pub bytes: Vec<u8>,
    pub text: String,
}

#[cfg(unix)]
trait CaptureStream: Read + std::os::fd::AsRawFd + Send {}
#[cfg(unix)]
impl<T: Read + std::os::fd::AsRawFd + Send> CaptureStream for T {}
#[cfg(windows)]
trait CaptureStream: Read + Send {}
#[cfg(windows)]
impl<T: Read + Send> CaptureStream for T {}

fn capture(
    mut stream: impl CaptureStream + 'static,
    deadline: Instant,
) -> mpsc::Receiver<Result<Vec<u8>, &'static str>> {
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut captured = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            if Instant::now() >= deadline {
                let _ = tx.send(Err("probe_capture_timeout"));
                return;
            }
            #[cfg(unix)]
            {
                let mut fd = libc::pollfd {
                    fd: stream.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                let ms = deadline
                    .saturating_duration_since(Instant::now())
                    .as_millis()
                    .min(i32::MAX as u128) as i32;
                let ready = unsafe { libc::poll(&mut fd, 1, ms) };
                if ready < 0 {
                    if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                        continue;
                    }
                    let _ = tx.send(Err("probe_capture_failed"));
                    return;
                }
                if ready == 0 {
                    let _ = tx.send(Err("probe_capture_timeout"));
                    return;
                }
            }
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let remaining = (MAX_OUTPUT_BYTES / 2).saturating_sub(captured.len());
                    captured.extend_from_slice(&buf[..n.min(remaining)]);
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    let _ = tx.send(Err("probe_capture_failed"));
                    return;
                }
            }
        }
        let _ = tx.send(Ok(captured));
    });
    rx
}

pub fn run(command: &str, cwd: &Path, limit: Duration) -> Result<Output, &'static str> {
    let deadline = Instant::now() + limit.min(PROBE_RUNTIME);
    #[cfg(unix)]
    let mut cmd = {
        use std::os::unix::process::CommandExt;
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", command]).process_group(0);
        cmd
    };
    #[cfg(windows)]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};
        let mut cmd = Command::new("powershell.exe");
        cmd.args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            command,
        ])
        .creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
        cmd
    };
    cmd.current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = cmd.spawn().map_err(|_| "probe_spawn_failed")?;
    let mut process = Process::new(child)?;
    let stdout = capture(
        process.child.stdout.take().ok_or("probe_capture_failed")?,
        deadline,
    );
    let stderr = capture(
        process.child.stderr.take().ok_or("probe_capture_failed")?,
        deadline,
    );
    let status = process
        .child
        .wait_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| "probe_wait_failed")?
        .ok_or("probe_timeout")?;
    process.terminate_descendants();
    let out = stdout
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| "probe_capture_timeout")??;
    let err = stderr
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| "probe_capture_timeout")??;
    let bytes = [out.as_slice(), err.as_slice()].concat();
    let text = format!(
        "exit={}\nstdout:\n{}\nstderr:\n{}",
        status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "signal".into()),
        String::from_utf8_lossy(&out),
        String::from_utf8_lossy(&err)
    );
    let mut hashed = status.code().unwrap_or(-1).to_le_bytes().to_vec();
    hashed.extend_from_slice(&(out.len() as u64).to_le_bytes());
    hashed.extend_from_slice(&bytes);
    Ok(Output {
        bytes: hashed,
        text,
    })
}

struct Process {
    child: Child,
    terminated: bool,
    #[cfg(windows)]
    job: windows_sys::Win32::Foundation::HANDLE,
}

impl Process {
    fn new(child: Child) -> Result<Self, &'static str> {
        #[cfg(windows)]
        {
            let mut child = child;
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Foundation::CloseHandle;
            use windows_sys::Win32::System::JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW,
            };
            let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if job.is_null()
                || unsafe { AssignProcessToJobObject(job, child.as_raw_handle() as _) } == 0
            {
                let _ = child.kill();
                let _ = child.wait();
                if !job.is_null() {
                    unsafe {
                        CloseHandle(job);
                    }
                }
                return Err("probe_containment_failed");
            }
            let process = Self {
                child,
                terminated: false,
                job,
            };
            super::probe_windows::resume(process.child.id())?;
            Ok(process)
        }
        #[cfg(unix)]
        {
            Ok(Self {
                child,
                terminated: false,
            })
        }
    }

    fn terminate_descendants(&mut self) {
        if self.terminated {
            return;
        }
        self.terminated = true;
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.job, 1);
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        self.terminate_descendants();
        let _ = self.child.kill();
        let _ = self.child.wait();
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.job);
        }
    }
}
