use super::{model::RuntimeFailure, rpc::Client};
use codeterm_plugin::agent_runtime::{AgentRuntimeCapability, AgentRuntimeProtocol};

fn transport_name(protocol: &AgentRuntimeProtocol) -> &'static str {
    match protocol {
        AgentRuntimeProtocol::StdioJsonrpc => "stdio-jsonrpc",
        AgentRuntimeProtocol::HttpSse => "http-sse",
    }
}
use parking_lot::Mutex;
use std::{collections::HashMap, path::Path, process::Stdio, sync::Arc, time::Duration};
use tokio::{io::AsyncReadExt, sync::watch};

pub struct Process {
    pub client: Client,
    pub ended: watch::Receiver<Option<RuntimeFailure>>,
    stop: watch::Sender<bool>,
}
impl Process {
    #[cfg(test)]
    pub(super) fn for_test(client: Client) -> Self {
        let (stop, _) = watch::channel(false);
        let ended = client.ended();
        Self {
            client,
            ended,
            stop,
        }
    }

    pub fn stop(&self) {
        self.stop.send_replace(true);
    }
    pub async fn finish(&self) -> Result<(), RuntimeFailure> {
        self.stop();
        let mut ended = self.ended.clone();
        tokio::time::timeout(Duration::from_secs(5), async {
            while ended.borrow_and_update().is_none() {
                ended.changed().await.map_err(|_| RuntimeFailure::PipeEof)?;
            }
            Ok(())
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

pub fn spawn(
    binary: &Path,
    cap: &AgentRuntimeCapability,
    cwd: &str,
    env: HashMap<String, String>,
    tab: &str,
) -> Result<Process, RuntimeFailure> {
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
    let job = crate::service::windows_job::Job::attach(&child)
        .map_err(|reason| RuntimeFailure::SpawnFailed { reason })?;
    let writer = child.stdin.take().ok_or(RuntimeFailure::PipeEof)?;
    let reader = child.stdout.take().ok_or(RuntimeFailure::PipeEof)?;
    let mut stderr = child.stderr.take().ok_or(RuntimeFailure::PipeEof)?;
    let output = Arc::new(Mutex::new(Vec::new()));
    let captured = output.clone();
    let stderr_reader = crate::async_runtime::spawn(async move {
        let mut bytes = [0u8; 4096];
        while let Ok(count) = stderr.read(&mut bytes).await {
            if count == 0 {
                break;
            }
            append_stderr(&mut captured.lock(), &bytes[..count]);
        }
    });
    let client = Client::attach(reader, writer);
    let dying = client.clone();
    let (stop, mut stopping) = watch::channel(false);
    let (ended, exit) = watch::channel(None);
    let process_tab = tab.to_string();
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
    crate::async_runtime::spawn(async move {
        #[cfg(unix)]
        let status = {
            let terminate = tokio::select! {_ = stopping.changed()=>true,result=&mut witnessed=>matches!(result,Ok(Ok(())))};
            if terminate {
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
                child.wait().await
            } else {
                tokio::select! {_ = stopping.changed()=>{unsafe{libc::kill(-(pid as i32),libc::SIGKILL);}child.wait().await},status=child.wait()=>status}
            }
        };
        #[cfg(windows)]
        let status = tokio::select! {_ = stopping.changed()=>{job.terminate();let _=child.kill().await;child.wait().await},status=child.wait()=>status};
        #[cfg(windows)]
        drop(job);
        #[cfg(unix)]
        drop(lease);
        let _ = tokio::time::timeout(Duration::from_secs(1), stderr_reader).await;
        let end = match status {
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
                    stderr: String::from_utf8_lossy(&output.lock()).into_owned(),
                    requested: *stopping.borrow(),
                }
            }
            Err(error) => RuntimeFailure::PipeFailure {
                stream: "process".into(),
                reason: error.to_string(),
            },
        };
        log::debug!("[agent-runtime] tab={process_tab} pid={pid} end={end:?}");
        dying.close(end.clone());
        ended.send_replace(Some(end));
    });
    Ok(Process {
        client,
        ended: exit,
        stop,
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
fn append_stderr(output: &mut Vec<u8>, bytes: &[u8]) {
    let bytes = &bytes[bytes.len().saturating_sub(8192)..];
    let retained = 8192 - bytes.len();
    output.drain(..output.len().saturating_sub(retained));
    output.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests;
