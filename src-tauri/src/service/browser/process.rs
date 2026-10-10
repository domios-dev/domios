use super::launch::{validate_loopback_endpoint, DevToolsListener};
use std::future::Future;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::sync::{oneshot, watch};
use tokio::time::Instant;

pub const READY_DEADLINE: Duration = Duration::from_secs(15);
pub const CLOSE_DEADLINE: Duration = Duration::from_secs(5);
pub const KILL_DEADLINE: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessExit {
    pub code: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseOutcome {
    AlreadyExited(ProcessExit),
    Graceful(ProcessExit),
    Killed(Option<ProcessExit>),
}

/// An owned Chromium child. Dropping it kills the whole process tree.
pub struct BrowserProcess {
    pid: u32,
    endpoint: String,
    ready_after: Duration,
    exit: watch::Receiver<Option<ProcessExit>>,
    kill: watch::Sender<bool>,
    #[cfg(windows)]
    _job: Option<crate::service::windows_job::Job>,
}

impl std::fmt::Debug for BrowserProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrowserProcess")
            .field("pid", &self.pid)
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

impl BrowserProcess {
    /// Returns once Chromium printed its DevTools endpoint; `deadline` only bounds failure.
    pub async fn spawn(binary: &Path, args: &[String], deadline: Duration) -> Result<Self, String> {
        Self::spawn_with_stdin(binary, args, deadline, Stdio::null()).await
    }

    async fn spawn_with_stdin(
        binary: &Path,
        args: &[String],
        deadline: Duration,
        stdin: Stdio,
    ) -> Result<Self, String> {
        let mut command = tokio::process::Command::new(binary);
        #[cfg(windows)]
        crate::process_util::no_window_tokio_suspended(&mut command);
        #[cfg(not(windows))]
        crate::process_util::no_window_tokio(&mut command);
        command
            .args(args)
            .stdin(stdin)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        // PDEATHSIG follows the spawning thread, so this must run on a long-lived runtime worker.
        #[cfg(target_os = "linux")]
        unsafe {
            command.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                Ok(())
            });
        }
        let started = Instant::now();
        let mut child = command
            .spawn()
            .map_err(|error| format!("browser spawn failed ({}): {error}", binary.display()))?;
        #[cfg(windows)]
        let job = contain(&child)?;
        let pid = child.id().ok_or("browser process has no pid")?;
        let stderr = child.stderr.take().ok_or("browser stderr is not piped")?;
        let stdout = child.stdout.take().ok_or("browser stdout is not piped")?;
        let (ready_tx, mut ready_rx) = oneshot::channel();
        tokio::spawn(drain_output(stderr, stdout, ready_tx));
        let (exit_tx, exit_rx) = watch::channel(None);
        let (kill_tx, kill_rx) = watch::channel(false);
        tokio::spawn(supervise(child, pid, kill_rx, exit_tx));
        let mut process = Self {
            pid,
            endpoint: String::new(),
            ready_after: Duration::ZERO,
            exit: exit_rx,
            kill: kill_tx,
            #[cfg(windows)]
            _job: job,
        };
        let mut exit = process.exit.clone();
        let readiness = tokio::time::timeout(deadline, async {
            tokio::select! {
                ready = &mut ready_rx => ready.unwrap_or_else(|_| Err("browser stderr reader stopped".into())),
                exited = exit.wait_for(Option::is_some) => {
                    let code = exited.ok().and_then(|exit| *exit).and_then(|exit| exit.code);
                    match ready_rx.await {
                        Ok(Err(detail)) => Err(format!("browser exited before DevTools was ready (code {code:?}): {detail}")),
                        _ => Err(format!("browser exited before DevTools was ready (code {code:?})")),
                    }
                }
            }
        })
        .await
        .map_err(|_| {
            format!(
                "browser did not report a DevTools endpoint within {}s",
                deadline.as_secs()
            )
        })??;
        validate_loopback_endpoint(&readiness, &["ws"])?;
        process.endpoint = readiness;
        process.ready_after = started.elapsed();
        Ok(process)
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn ready_after(&self) -> Duration {
        self.ready_after
    }

    pub fn exit_status(&self) -> Option<ProcessExit> {
        *self.exit.borrow()
    }

    pub fn exit_watch(&self) -> watch::Receiver<Option<ProcessExit>> {
        self.exit.clone()
    }

    pub fn kill(&self) {
        self.kill.send_replace(true);
    }

    pub async fn wait_exit_until(&self, until: Instant) -> Option<ProcessExit> {
        let mut exit = self.exit.clone();
        if let Ok(Ok(value)) = tokio::time::timeout_at(until, exit.wait_for(Option::is_some)).await
        {
            return *value;
        }
        self.exit_status()
    }

    /// `request_close` asks Chromium to quit (CDP `Browser.close`) so it flushes the profile first.
    pub async fn close_with<F>(&self, request_close: F, deadline: Duration) -> CloseOutcome
    where
        F: Future<Output = ()>,
    {
        if let Some(exit) = self.exit_status() {
            return CloseOutcome::AlreadyExited(exit);
        }
        let until = Instant::now() + deadline;
        let _ = tokio::time::timeout_at(until, request_close).await;
        if let Some(exit) = self.wait_exit_until(until).await {
            return CloseOutcome::Graceful(exit);
        }
        self.kill();
        CloseOutcome::Killed(self.wait_exit_until(Instant::now() + KILL_DEADLINE).await)
    }
}

impl Drop for BrowserProcess {
    fn drop(&mut self) {
        self.kill.send_replace(true);
    }
}

/// Every Chromium process descends from the suspended root, so all of them die with the daemon.
#[cfg(windows)]
fn contain(
    child: &tokio::process::Child,
) -> Result<Option<crate::service::windows_job::Job>, String> {
    let job = crate::service::windows_job::Job::contain(child)
        .inspect_err(|error| log::warn!("[browser-process] no kill-on-close job: {error}"))
        .ok();
    crate::service::windows_job::resume(child)?;
    Ok(job)
}

async fn drain_output(
    mut stderr: tokio::process::ChildStderr,
    mut stdout: tokio::process::ChildStdout,
    ready: oneshot::Sender<Result<String, String>>,
) {
    let mut stderr_listener = DevToolsListener::default();
    let mut stdout_listener = DevToolsListener::default();
    let mut ready = Some(ready);
    let mut stderr_buffer = vec![0u8; 8192];
    let mut stdout_buffer = vec![0u8; 8192];
    let mut stderr_open = true;
    let mut stdout_open = true;
    while stderr_open || stdout_open {
        let (is_stderr, read) = tokio::select! {
            read = stderr.read(&mut stderr_buffer), if stderr_open => (true, read),
            read = stdout.read(&mut stdout_buffer), if stdout_open => (false, read),
        };
        let (listener, buffer, open) = if is_stderr {
            (&mut stderr_listener, &stderr_buffer, &mut stderr_open)
        } else {
            (&mut stdout_listener, &stdout_buffer, &mut stdout_open)
        };
        let endpoint = match read {
            Ok(0) | Err(_) => {
                *open = false;
                listener.finish()
            }
            Ok(read) => listener.push(&buffer[..read]),
        };
        if let Some(endpoint) = endpoint {
            if let Some(ready) = ready.take() {
                let _ = ready.send(Ok(endpoint));
            }
        }
    }
    if let Some(ready) = ready.take() {
        let _ = ready.send(Err(format!(
            "browser closed output before DevTools was ready: {} | {}",
            stderr_listener.tail(),
            stdout_listener.tail()
        )));
    }
}

async fn supervise(
    mut child: tokio::process::Child,
    pid: u32,
    mut kill: watch::Receiver<bool>,
    exit: watch::Sender<Option<ProcessExit>>,
) {
    // The watch Ref must not live into the kill branch's awaits: it holds the channel's read lock.
    let kill_requested = async move {
        let _ = kill.wait_for(|kill| *kill).await;
    };
    // Child::wait closes its stdin; the supervisor keeps the pipe alive until exit.
    let _stdin = child.stdin.take();
    let status = tokio::select! {
        status = child.wait() => status.ok(),
        () = kill_requested => {
            kill_tree(pid).await;
            let _ = child.start_kill();
            child.wait().await.ok()
        }
    };
    exit.send_replace(Some(ProcessExit {
        code: status.and_then(|status| status.code()),
    }));
}

/// The tree kill runs before the child handle kill so the parent link still finds the children.
async fn kill_tree(pid: u32) {
    match tokio::task::spawn_blocking(move || super::sweep::kill_tree_blocking(pid)).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => log::warn!("[browser-process] tree kill {pid}: {error}"),
        Err(error) => log::warn!("[browser-process] tree kill {pid} task failed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAKE_ENDPOINT: &str = "ws://127.0.0.1:9/devtools/browser/fake";

    #[cfg(windows)]
    fn fake_browser(stderr_line: &str, hold: bool) -> (&'static str, Vec<String>) {
        let hold = if hold {
            "; [Console]::In.ReadLine() | Out-Null"
        } else {
            ""
        };
        (
            "powershell",
            vec![
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-Command".into(),
                format!("[Console]::Error.WriteLine('{stderr_line}'){hold}"),
            ],
        )
    }

    #[cfg(unix)]
    fn fake_browser(stderr_line: &str, hold: bool) -> (&'static str, Vec<String>) {
        let hold = if hold { "; read line" } else { "" };
        (
            "sh",
            vec!["-c".into(), format!("echo '{stderr_line}' >&2{hold}")],
        )
    }

    #[tokio::test]
    async fn browser_process_ready_from_stderr_then_killed_on_request() {
        let (binary, args) = fake_browser(&format!("DevTools listening on {FAKE_ENDPOINT}"), true);
        let process = BrowserProcess::spawn_with_stdin(
            Path::new(binary),
            &args,
            READY_DEADLINE,
            Stdio::piped(),
        )
        .await
        .unwrap();
        assert_eq!(process.endpoint(), FAKE_ENDPOINT);
        assert_eq!(process.exit_status(), None);
        let outcome = process.close_with(async {}, Duration::ZERO).await;
        assert!(
            matches!(outcome, CloseOutcome::Killed(Some(_))),
            "{outcome:?}"
        );
    }

    #[tokio::test]
    async fn browser_process_exit_before_devtools_is_an_error() {
        let (binary, args) = fake_browser("Opening in existing browser session.", false);
        let error = BrowserProcess::spawn(Path::new(binary), &args, READY_DEADLINE)
            .await
            .unwrap_err();
        assert!(error.contains("before DevTools was ready"), "{error}");
    }

    #[tokio::test]
    async fn browser_process_foreign_profile_handoff_on_stdout_keeps_diagnostic() {
        let (binary, args) = fake_browser("Opening in existing browser session.", false);
        let args: Vec<String> = args
            .into_iter()
            .map(|arg| {
                arg.replace("[Console]::Error", "[Console]")
                    .replace(" >&2", "")
            })
            .collect();
        let error = BrowserProcess::spawn(Path::new(binary), &args, READY_DEADLINE)
            .await
            .unwrap_err();
        let dir = Path::new("/profiles/foreign");
        let error = super::super::profile::foreign_profile_error(dir, &error);
        assert!(error.contains(&dir.display().to_string()), "{error}");
        assert!(error.contains("CodeTerm does not own"), "{error}");
    }

    #[tokio::test]
    async fn browser_process_rejects_non_loopback_endpoint() {
        let (binary, args) = fake_browser(
            "DevTools listening on ws://10.0.0.5:9222/devtools/browser/x",
            true,
        );
        let error = BrowserProcess::spawn_with_stdin(
            Path::new(binary),
            &args,
            READY_DEADLINE,
            Stdio::piped(),
        )
        .await
        .unwrap_err();
        assert!(error.contains("loopback"), "{error}");
    }

    #[tokio::test]
    async fn browser_process_missing_binary_fails_fast() {
        let error =
            BrowserProcess::spawn(Path::new("/definitely/not/a/browser"), &[], READY_DEADLINE)
                .await
                .unwrap_err();
        assert!(error.starts_with("browser spawn failed"), "{error}");
    }
}
