use super::{model::RuntimeFailure, rpc::Client};
use codeterm_plugin::agent_runtime::AgentRuntimeProtocol;

fn transport_name(protocol: &AgentRuntimeProtocol) -> &'static str {
    match protocol {
        AgentRuntimeProtocol::StdioJsonrpc => "stdio-jsonrpc",
        AgentRuntimeProtocol::HttpSse => "http-sse",
    }
}
use parking_lot::Mutex;
use std::{collections::HashMap, process::Stdio, sync::Arc, time::Duration};
use tokio::{io::AsyncReadExt, sync::watch};

pub struct Process {
    pub client: Client,
    pub ended: watch::Receiver<Option<RuntimeFailure>>,
    stop: watch::Sender<Option<bool>>,
    turn_members: parking_lot::Mutex<Vec<super::process_boundary::Member>>,
    boundary: Option<Arc<super::process_boundary::Boundary>>,
}
impl Process {
    #[cfg(test)]
    pub(super) fn for_test(client: Client) -> Self {
        let (stop, mut stopping) = watch::channel(None::<bool>);
        let (ended, exit) = watch::channel(None);
        let dying = client.clone();
        crate::async_runtime::spawn(async move {
            let _ = stopping.changed().await;
            let reason = RuntimeFailure::ProcessExited {
                code: Some(0),
                signal: None,
                stderr: String::new(),
                requested: stopping.borrow().unwrap_or(false),
            };
            dying.close(reason.clone());
            ended.send_replace(Some(reason));
        });
        Self {
            client,
            ended: exit,
            stop,
            boundary: None,
            turn_members: parking_lot::Mutex::new(Vec::new()),
        }
    }

    pub(super) async fn observe(
        &self,
    ) -> Result<Vec<super::process_boundary::Member>, RuntimeFailure> {
        let Some(boundary) = self.boundary.clone() else {
            return Ok(Vec::new());
        };
        tokio::task::spawn_blocking(move || boundary.observe())
            .await
            .map_err(|error| RuntimeFailure::HookFailed(error.to_string()))?
    }

    pub(super) async fn begin_turn(&self) -> Result<(), RuntimeFailure> {
        let members = self.observe().await?;
        *self.turn_members.lock() = members;
        Ok(())
    }

    pub(super) async fn verify_interrupt(
        &self,
        captured: Vec<super::process_boundary::Member>,
        roster: Vec<codeterm_plugin::agent_runtime::RuntimeBackgroundProcess>,
    ) -> Result<Vec<u32>, RuntimeFailure> {
        let Some(boundary) = self.boundary.clone() else {
            return Ok(Vec::new());
        };
        let baseline = std::mem::take(&mut *self.turn_members.lock());
        tokio::task::spawn_blocking(move || boundary.verify_interrupt(captured, &baseline, &roster))
            .await
            .map_err(|error| RuntimeFailure::HookFailed(error.to_string()))?
    }

    pub(super) fn terminated(&self) -> bool {
        matches!(
            *self.ended.borrow(),
            Some(RuntimeFailure::ProcessExited { .. })
        )
    }

    pub fn stop(&self) {
        self.signal_stop(true);
    }
    fn signal_stop(&self, requested: bool) {
        signal_stop(&self.stop, requested);
    }
    pub async fn finish(&self) -> Result<(), RuntimeFailure> {
        self.finish_with_intent(true).await
    }
    pub(super) async fn finish_after_failure(&self) -> Result<(), RuntimeFailure> {
        self.finish_with_intent(false).await
    }
    async fn finish_with_intent(&self, requested: bool) -> Result<(), RuntimeFailure> {
        self.signal_stop(requested);
        let mut ended = self.ended.clone();
        tokio::time::timeout(Duration::from_secs(10), async {
            while ended.borrow_and_update().is_none() {
                ended.changed().await.map_err(|_| RuntimeFailure::PipeEof)?;
            }
            match ended.borrow().clone() {
                Some(RuntimeFailure::ProcessExited { .. }) => Ok(()),
                Some(reason) => Err(reason),
                None => Err(RuntimeFailure::PipeEof),
            }
        })
        .await
        .map_err(|_| RuntimeFailure::Deadline)?
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        self.client.close(RuntimeFailure::Superseded);
        self.stop();
    }
}

pub(super) fn spawn(
    launch: &super::launch_profile::ResolvedLaunch,
    tab_cwd: &str,
    env: HashMap<String, String>,
    _tab: &str,
) -> Result<Process, RuntimeFailure> {
    let (binary, cap) = (launch.binary.as_path(), &launch.capability);
    let cwd = launch.process_cwd(tab_cwd);
    #[cfg(test)]
    if let Some(factory) = test_ports::factory(_tab) {
        test_ports::record_spawn(_tab, &env, cwd);
        return Ok(factory());
    }
    if cap.runtime != codeterm_plugin::agent_runtime::AgentRuntimeProtocol::StdioJsonrpc {
        return Err(RuntimeFailure::SpawnFailed {
            reason: format!(
                "Runtime transport {} has no reader",
                transport_name(&cap.runtime)
            ),
        });
    }
    if let Some(problem) = crate::service::spawn_cwd::cwd_problem(cwd) {
        return Err(RuntimeFailure::SpawnFailed {
            reason: format!("Runtime cwd {cwd:?}: {problem}"),
        });
    }
    let mut command = tokio::process::Command::from(
        codeterm_plugin::provider_command::command(binary)
            .map_err(|reason| RuntimeFailure::SpawnFailed { reason })?,
    );
    #[cfg(windows)]
    crate::process_util::no_window_tokio_suspended(&mut command);
    for key in codeterm_pty::pty::manager::INHERITED_ORCHESTRATION_ENV {
        command.env_remove(key);
    }
    command.envs(&env);
    if let Some(cli) = env.get("CODETERM_CLI_PATH") {
        if let Some(dir) = codeterm_pty::pty::manager::cli_dir_of(std::ffi::OsStr::new(cli)) {
            let inherited = env
                .get("PATH")
                .map(std::ffi::OsString::from)
                .or_else(|| std::env::var_os("PATH"))
                .unwrap_or_default();
            if let Some(path) =
                codeterm_pty::pty::manager::path_with_cli_dir_first(&inherited, &dir)
            {
                command.env("PATH", path);
            }
        }
    }
    let effective = effective_env(command.as_std());
    let credential = effective
        .get("CODETERM_API_TOKEN")
        .cloned()
        .unwrap_or_default();
    let exclusions = cap
        .inherited_environment_exclusions
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    for (key, value) in codeterm_pty::pty::child_env::child_env_changes_with_exclusions(
        effective
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str())),
        &exclusions,
    ) {
        match value {
            Some(value) => command.env(key, value),
            None => command.env_remove(key),
        };
    }
    command
        .args(&cap.command.args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    let lease = super::lease::Lease::configure(&mut command).map_err(|error| {
        RuntimeFailure::SpawnFailed {
            reason: error.to_string(),
        }
    })?;
    let mut child = command
        .spawn()
        .map_err(|error| RuntimeFailure::SpawnFailed {
            reason: error.to_string(),
        })?;
    let pid = child.id().ok_or_else(|| RuntimeFailure::SpawnFailed {
        reason: "Process id unavailable".into(),
    })?;
    #[cfg(windows)]
    let job = Arc::new(
        crate::service::windows_job::Job::contain(&child)
            .map_err(|reason| RuntimeFailure::SpawnFailed { reason })?,
    );
    let boundary = Arc::new(super::process_boundary::Boundary::new(
        pid,
        #[cfg(windows)]
        job.clone(),
    )?);
    #[cfg(windows)]
    crate::service::windows_job::resume(&child)
        .map_err(|reason| RuntimeFailure::SpawnFailed { reason })?;
    let writer = child.stdin.take().ok_or(RuntimeFailure::PipeEof)?;
    let reader = child.stdout.take().ok_or(RuntimeFailure::PipeEof)?;
    let mut stderr = child.stderr.take().ok_or(RuntimeFailure::PipeEof)?;
    let output = Arc::new(Mutex::new(Vec::new()));
    let captured = output.clone();
    let stderr_credential = credential.clone();
    let stderr_reader = crate::async_runtime::spawn(async move {
        let mut bytes = [0u8; 4096];
        let mut pending = Vec::new();
        while let Ok(count) = stderr.read(&mut bytes).await {
            if count == 0 {
                break;
            }
            append_stderr(
                &mut captured.lock(),
                &bytes[..count],
                stderr_credential.as_bytes(),
                &mut pending,
            );
        }
    });
    let client = Client::attach_redacted(reader, writer, cap.envelope.clone(), credential.clone());
    let dying = client.clone();
    let (stop, mut stopping) = watch::channel(None::<bool>);
    let (ended, exit) = watch::channel(None);
    #[cfg(debug_assertions)]
    let process_tab = _tab.to_string();
    #[cfg(unix)]
    let mut witnessed = tokio::task::spawn_blocking(move || {
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        loop {
            if unsafe { libc::waitid(libc::P_PID, pid, &mut info, libc::WEXITED | libc::WNOWAIT) }
                == 0
            {
                return Ok(());
            }
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::EINTR) {
                return Err(error);
            }
        }
    });
    let retiring = boundary.clone();
    crate::async_runtime::spawn(async move {
        #[cfg(unix)]
        let requested = tokio::select! {
            biased;
            _ = &mut witnessed => false,
            _ = stopping.changed() => stopping.borrow().unwrap_or(false),
        };
        #[cfg(windows)]
        let requested = tokio::select! {
            biased;
            _ = child.wait() => false,
            _ = stopping.changed() => stopping.borrow().unwrap_or(false),
        };
        // Cleanup may request termination after pipe EOF; it cannot rewrite the observed cause.
        let retirement = {
            let owned = retiring.clone();
            tokio::task::spawn_blocking(move || owned.retire()).await
        };
        let _ = retiring.force_terminate();
        let status = child.wait().await;
        #[cfg(unix)]
        drop(lease);
        let retirement = retirement
            .map_err(|error| RuntimeFailure::HookFailed(error.to_string()))
            .and_then(|result| result);
        let _ = tokio::time::timeout(Duration::from_secs(1), stderr_reader).await;
        let end = if let Err(reason) = retirement {
            reason
        } else {
            match status {
                Ok(status) => {
                    #[cfg(unix)]
                    let signal = {
                        use std::os::unix::process::ExitStatusExt;
                        status.signal()
                    };
                    #[cfg(not(unix))]
                    let signal = None;
                    RuntimeFailure::ProcessExited {
                        code: status.code(),
                        signal,
                        stderr: {
                            let text = String::from_utf8_lossy(&output.lock()).into_owned();
                            if credential.is_empty() {
                                text
                            } else {
                                text.replace(&credential, "[redacted]")
                            }
                        },
                        requested,
                    }
                }
                Err(error) => RuntimeFailure::PipeFailure {
                    stream: "process".into(),
                    reason: error.to_string(),
                },
            }
        };
        #[cfg(debug_assertions)]
        match &end {
            RuntimeFailure::ProcessExited { code, signal, requested, .. } =>
                log::debug!("[runtime-policy] phase=child_exit tab={process_tab} pid={pid} requested={requested} code={code:?} signal={signal:?}"),
            _ => log::debug!("[runtime-policy] phase=child_exit tab={process_tab} pid={pid} outcome=retirement_or_wait_failure"),
        }
        dying.close(end.clone());
        ended.send_replace(Some(end));
    });
    Ok(Process {
        client,
        ended: exit,
        stop,
        boundary: Some(boundary),
        turn_members: parking_lot::Mutex::new(Vec::new()),
    })
}
fn effective_env(command: &std::process::Command) -> HashMap<String, String> {
    let mut env = std::env::vars_os()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect::<HashMap<_, _>>();
    for (key, value) in command.get_envs() {
        let key = key.to_string_lossy().into_owned();
        match value {
            Some(value) => {
                env.insert(key, value.to_string_lossy().into_owned());
            }
            None => {
                env.remove(&key);
            }
        }
    }
    env
}
fn append_stderr(output: &mut Vec<u8>, bytes: &[u8], credential: &[u8], pending: &mut Vec<u8>) {
    if credential.is_empty() {
        append_stderr_tail(output, bytes);
        return;
    }
    pending.extend_from_slice(bytes);
    let mut start = 0;
    while let Some(offset) = pending[start..]
        .windows(credential.len())
        .position(|window| window == credential)
    {
        let end = start + offset;
        append_stderr_tail(output, &pending[start..end]);
        append_stderr_tail(output, b"[redacted]");
        start = end + credential.len();
    }
    let overlap = (1..credential.len().min(pending.len() - start + 1))
        .rev()
        .find(|&length| pending[start..].ends_with(&credential[..length]))
        .unwrap_or(0);
    let end = pending.len() - overlap;
    append_stderr_tail(output, &pending[start..end]);
    pending.drain(..end);
}

fn append_stderr_tail(output: &mut Vec<u8>, bytes: &[u8]) {
    let bytes = &bytes[bytes.len().saturating_sub(8192)..];
    let retained = 8192 - bytes.len();
    output.drain(..output.len().saturating_sub(retained));
    output.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) mod test_ports;

fn signal_stop(stop: &watch::Sender<Option<bool>>, requested: bool) {
    stop.send_if_modified(|intent| {
        if intent.is_some() {
            return false;
        }
        *intent = Some(requested);
        true
    });
}
