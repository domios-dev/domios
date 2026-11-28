use super::launch::{validate_loopback_endpoint, DevToolsListener};
use std::future::Future;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::sync::{oneshot, watch};
use tokio::time::Instant;

pub mod termination;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(unix)]
mod unix_group;

pub const READY_DEADLINE: Duration = Duration::from_secs(15);
pub const CLOSE_DEADLINE: Duration = Duration::from_secs(5);
pub const KILL_DEADLINE: Duration = Duration::from_secs(5);
#[cfg(target_os = "macos")]
pub const FORCE_TERMINATE_DEADLINE: Duration = Duration::from_secs(5);

fn forced_exit_window() -> Duration {
    #[cfg(target_os = "macos")]
    {
        FORCE_TERMINATE_DEADLINE + KILL_DEADLINE
    }
    #[cfg(not(target_os = "macos"))]
    {
        KILL_DEADLINE
    }
}

#[derive(Debug)]
pub struct SpawnError {
    pub cause: SpawnFailure,
    pub process_may_be_running: bool,
}

#[derive(Debug)]
pub enum SpawnFailure {
    Setup(String),
    #[cfg(windows)]
    JobContainment(String),
    ExitedBeforeDevTools {
        exit: ProcessExit,
        output: String,
    },
    OutputClosed(String),
    OutputReaderStopped,
    ReadinessDeadline(Duration),
    InvalidEndpoint(String),
}

impl SpawnError {
    fn before_launch(detail: String) -> Self {
        Self {
            cause: SpawnFailure::Setup(detail),
            process_may_be_running: false,
        }
    }
}

impl std::fmt::Display for SpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.cause {
            SpawnFailure::Setup(detail) => f.write_str(detail),
            #[cfg(windows)]
            SpawnFailure::JobContainment(detail) => write!(
                f,
                "browser_job_containment_failed: could not contain or resume Chromium in its kill-on-close Job: {detail}"
            ),
            SpawnFailure::ExitedBeforeDevTools { exit, output } => {
                f.write_str("browser_exited_before_devtools: browser exited before DevTools was ready")?;
                match exit.code {
                    Some(code) => write!(f, " (exit code {code})")?,
                    None => f.write_str(" (no numeric exit code)")?,
                }
                if output.is_empty() {
                    f.write_str("; no startup output was captured")
                } else {
                    write!(f, ": {output}")
                }
            }
            SpawnFailure::OutputClosed(output) => {
                f.write_str("browser_output_closed_before_devtools: browser closed output before DevTools was ready")?;
                if output.is_empty() {
                    Ok(())
                } else {
                    write!(f, ": {output}")
                }
            }
            SpawnFailure::OutputReaderStopped => {
                f.write_str("browser_output_reader_stopped: browser output reader stopped before DevTools was ready")
            }
            SpawnFailure::ReadinessDeadline(deadline) => write!(
                f,
                "browser_devtools_timeout: browser did not report a DevTools endpoint within {}s",
                deadline.as_secs()
            ),
            SpawnFailure::InvalidEndpoint(detail) => {
                write!(f, "browser_invalid_devtools_endpoint: {detail}")
            }
        }
    }
}

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

impl CloseOutcome {
    pub fn confirms_exit(self) -> bool {
        !matches!(self, Self::Killed(None))
    }
}

pub struct BrowserProcess {
    pid: u32,
    endpoint: String,
    ready_after: Duration,
    exit: watch::Receiver<Option<ProcessExit>>,
    kill: watch::Sender<bool>,
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
    pub async fn spawn(
        binary: &Path,
        args: &[String],
        deadline: Duration,
    ) -> Result<Self, SpawnError> {
        #[cfg(target_os = "macos")]
        {
            let resolved = binary.to_path_buf();
            let resolved = tokio::task::spawn_blocking(move || {
                std::fs::canonicalize(&resolved).unwrap_or(resolved)
            })
            .await
            .map_err(|error| SpawnError::before_launch(error.to_string()))?;
            if let Some(spec) =
                crate::launch::macos_app_launch(crate::locator::HostOs::MacOs, &resolved, args)
                    .map_err(SpawnError::before_launch)?
            {
                return macos::spawn(&resolved, args, spec, deadline).await;
            }
        }
        Self::spawn_with_stdin(binary, args, deadline, Stdio::null()).await
    }

    async fn spawn_with_stdin(
        binary: &Path,
        args: &[String],
        deadline: Duration,
        stdin: Stdio,
    ) -> Result<Self, SpawnError> {
        let mut command = tokio::process::Command::new(binary);
        #[cfg(windows)]
        codeterm_util::process_util::no_window_tokio_suspended(&mut command);
        #[cfg(not(windows))]
        codeterm_util::process_util::no_window_tokio(&mut command);
        command
            .args(args)
            .stdin(stdin)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        let started = Instant::now();
        let mut child = command.spawn().map_err(|error| {
            SpawnError::before_launch(format!(
                "browser spawn failed ({}): {error}",
                binary.display()
            ))
        })?;
        #[cfg(windows)]
        let tree = match codeterm_util::windows_job::Job::attach(&child) {
            Ok(tree) => tree,
            Err(detail) => {
                let _ = child.start_kill();
                let exited = tokio::time::timeout(KILL_DEADLINE, child.wait()).await;
                return Err(SpawnError {
                    cause: SpawnFailure::JobContainment(detail),
                    process_may_be_running: !matches!(exited, Ok(Ok(_))),
                });
            }
        };
        let pid = child
            .id()
            .ok_or_else(|| SpawnError::before_launch("browser process has no pid".into()))?;
        #[cfg(unix)]
        let tree = unix_group::OwnedProcessGroup::new(pid).map_err(|error| {
            SpawnError::before_launch(format!("could not guard Chromium process group: {error}"))
        })?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| SpawnError::before_launch("browser stderr is not piped".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| SpawnError::before_launch("browser stdout is not piped".into()))?;
        let (ready_tx, ready_rx) = oneshot::channel();
        tokio::spawn(drain_output(stderr, stdout, ready_tx));
        let (exit_tx, exit_rx) = watch::channel(None);
        let (kill_tx, kill_rx) = watch::channel(false);
        tokio::spawn(supervise(child, tree, kill_rx, exit_tx));
        let mut process = Self {
            pid,
            endpoint: String::new(),
            ready_after: Duration::ZERO,
            exit: exit_rx,
            kill: kill_tx,
        };
        match process
            .await_devtools(ready_rx, started + deadline, deadline)
            .await
        {
            Ok(endpoint) => process.endpoint = endpoint,
            Err(cause) => {
                process.kill();
                let exited = process
                    .wait_exit_until(Instant::now() + KILL_DEADLINE)
                    .await;
                return Err(SpawnError {
                    cause,
                    process_may_be_running: exited.is_none(),
                });
            }
        }
        process.ready_after = started.elapsed();
        Ok(process)
    }

    async fn await_devtools(
        &self,
        ready: oneshot::Receiver<Result<String, String>>,
        until: Instant,
        deadline: Duration,
    ) -> Result<String, SpawnFailure> {
        let (failure, exit) = match tokio::time::timeout_at(until, ready).await {
            Ok(Ok(Ok(endpoint))) => {
                validate_loopback_endpoint(&endpoint, &["ws"])
                    .map_err(SpawnFailure::InvalidEndpoint)?;
                return Ok(endpoint);
            }
            Ok(Ok(Err(output))) => {
                return Err(match self.wait_exit_until(until).await {
                    Some(exit) => SpawnFailure::ExitedBeforeDevTools { exit, output },
                    None => SpawnFailure::OutputClosed(output),
                });
            }
            Ok(Err(_)) => (SpawnFailure::OutputReaderStopped, self.exit_status()),
            Err(_) => (
                SpawnFailure::ReadinessDeadline(deadline),
                self.exit_status(),
            ),
        };
        Err(match exit {
            Some(exit) => SpawnFailure::ExitedBeforeDevTools {
                exit,
                output: String::new(),
            },
            None => failure,
        })
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
        CloseOutcome::Killed(
            self.wait_exit_until(Instant::now() + forced_exit_window())
                .await,
        )
    }
}

impl Drop for BrowserProcess {
    fn drop(&mut self) {
        self.kill.send_replace(true);
    }
}

#[cfg(windows)]
type ProcessTree = codeterm_util::windows_job::Job;
#[cfg(unix)]
type ProcessTree = unix_group::OwnedProcessGroup;

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
        let output = [stderr_listener.tail(), stdout_listener.tail()]
            .into_iter()
            .filter(|tail| !tail.is_empty())
            .collect::<Vec<_>>()
            .join(" | ");
        let _ = ready.send(Err(output));
    }
}

async fn supervise(
    mut child: tokio::process::Child,
    tree: ProcessTree,
    mut kill: watch::Receiver<bool>,
    exit: watch::Sender<Option<ProcessExit>>,
) {
    let kill_requested = async move {
        let _ = kill.wait_for(|kill| *kill).await;
    };
    // Child::wait closes its stdin; the supervisor keeps the pipe alive until exit.
    let _stdin = child.stdin.take();
    #[cfg(windows)]
    let status = {
        tokio::select! {
            _ = kill_requested => {}
            _ = child.wait() => {}
        }
        let reaped =
            tokio::task::spawn_blocking(move || tree.terminate_and_wait(KILL_DEADLINE)).await;
        match reaped {
            Ok(Ok(())) => child.wait().await,
            Ok(Err(error)) => Err(std::io::Error::other(error)),
            Err(error) => Err(std::io::Error::other(error)),
        }
    };
    #[cfg(unix)]
    let status = tree.reap(&mut child, kill_requested).await;
    match status {
        Ok(status) => {
            exit.send_replace(Some(ProcessExit {
                code: status.code(),
            }));
        }
        Err(error) => log::warn!("[browser-process] tree exit unconfirmed: {error}"),
    }
}

#[cfg(test)]
mod tests;
