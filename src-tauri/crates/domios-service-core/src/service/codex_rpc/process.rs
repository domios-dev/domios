use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::sync::{oneshot, watch};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Marker {
    pub owner_pid: u32,
    pub owner_start_token: String,
    pub instance: String,
    pub tab: String,
    pub pid: u32,
    pub start_token: String,
    pub binary: PathBuf,
}

pub fn identity_matches(marker: &Marker, instance: &str) -> bool {
    marker.instance == instance
        && marker.pid > 1
        && crate::service::process_tree::launch_processes(marker.pid)
            .first()
            .is_some_and(|process| {
                process.pid == marker.pid && process.start_token == marker.start_token
            })
}

pub struct Process {
    stop: watch::Sender<bool>,
    pub death: watch::Receiver<bool>,
}

impl Drop for Process {
    fn drop(&mut self) {
        self.stop.send_replace(true);
    }
}
impl Process {
    pub fn stop(&self) {
        self.stop.send_replace(true);
    }

    pub(super) fn cleanup(&self) -> Cleanup {
        Cleanup {
            stop: self.stop.clone(),
            death: self.death.clone(),
        }
    }
}

pub(super) struct Cleanup {
    stop: watch::Sender<bool>,
    death: watch::Receiver<bool>,
}

impl Cleanup {
    pub async fn finish(mut self) -> Result<(), String> {
        self.stop.send_replace(true);
        tokio::time::timeout(Duration::from_secs(5), async {
            while !*self.death.borrow_and_update() {
                self.death
                    .changed()
                    .await
                    .map_err(|_| "Sidecar cleanup channel closed")?;
            }
            Ok(())
        })
        .await
        .map_err(|_| "Sidecar cleanup deadline")?
    }
}

#[cfg(unix)]
fn observe_exit(mut wait: impl FnMut() -> std::io::Result<()>) -> std::io::Result<()> {
    loop {
        match wait() {
            Err(error) if error.raw_os_error() == Some(libc::EINTR) => continue,
            result => return result,
        }
    }
}

const INHERITED_PROVIDER_IDENTITY: &[&str] = &["CODEX_THREAD_ID", "CODEX_SESSION_ID", "CODEX_CI"];

fn effective_env(command: &std::process::Command) -> Vec<(String, String)> {
    let mut env: std::collections::BTreeMap<String, String> = std::env::vars_os()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect();
    for (key, value) in command.get_envs() {
        let key = key.to_string_lossy().into_owned();
        match value {
            Some(value) => env.insert(key, value.to_string_lossy().into_owned()),
            None => env.remove(&key),
        };
    }
    env.into_iter().collect()
}

pub async fn spawn(
    binary: &Path,
    cwd: &str,
    env: std::collections::HashMap<String, String>,
    tab: &str,
    port: u16,
    token_file: &Path,
    marker_file: &Path,
) -> Result<Process, String> {
    if let Some(problem) = crate::service::spawn_cwd::cwd_problem(cwd) {
        return Err(format!("Sidecar cwd {cwd:?}: {problem}"));
    }
    let mut command =
        tokio::process::Command::from(codeterm_plugin::provider_command::command(binary)?);
    #[cfg(windows)]
    crate::process_util::no_window_tokio_suspended(&mut command);
    for key in codeterm_pty::pty::manager::INHERITED_ORCHESTRATION_ENV {
        command.env_remove(key);
    }
    command.envs(&env);
    command.env("CODETERM", "1");
    for key in codeterm_pty::pty::manager::STRIPPED_PTY_ENV {
        command.env_remove(key);
    }
    if let Some(cli) = env
        .get("CODETERM_CLI_PATH")
        .map(std::ffi::OsString::from)
        .or_else(|| std::env::var_os("CODETERM_CLI_PATH"))
    {
        if let Some(dir) = codeterm_pty::pty::manager::cli_dir_of(&cli) {
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
    let effective = effective
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()));
    let changes = codeterm_pty::pty::child_env::child_env_changes_with_exclusions(
        effective,
        INHERITED_PROVIDER_IDENTITY,
    );
    let removed: Vec<_> = changes
        .iter()
        .filter(|(_, value)| value.is_none())
        .map(|(key, _)| key)
        .collect();
    log::debug!("[codex-rpc-env] tab={tab} removed_keys={removed:?}");
    for (key, value) in changes {
        match value {
            Some(value) => command.env(key, value),
            None => command.env_remove(key),
        };
    }
    command
        .args([
            "app-server",
            "-c",
            "project_doc_max_bytes=65536",
            "--listen",
            &format!("ws://127.0.0.1:{port}"),
            "--ws-auth",
            "capability-token",
            "--ws-token-file",
        ])
        .arg(token_file)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    let lease = super::unix_lease::Lease::configure(&mut command)
        .map_err(|error| format!("Sidecar lifetime lease: {error}"))?;
    let mut child = command.spawn().map_err(|error| {
        format!(
            "Sidecar process spawn failed: binary={binary:?} cwd={cwd:?} token_file={token_file:?} CODEX_HOME={:?}: {error}",
            env.get("CODEX_HOME").cloned().or_else(|| std::env::var("CODEX_HOME").ok())
        )
    })?;
    let pid = child.id().ok_or("Sidecar has no process id")?;
    #[cfg(windows)]
    let job = crate::service::windows_job::Job::attach(&child)?;
    let (ready, readiness) = oneshot::channel();
    let ready = std::sync::Arc::new(parking_lot::Mutex::new(Some(ready)));
    let output = std::sync::Arc::new(parking_lot::Mutex::new(
        super::process_output::Output::default(),
    ));
    let expected = format!("listening on: ws://127.0.0.1:{port}");
    let stdout = child.stdout.take().ok_or("Missing sidecar stdout")?;
    let stderr = child.stderr.take().ok_or("Missing sidecar stderr")?;
    let ready_stdout = ready.clone();
    let expected_stdout = expected.clone();
    crate::async_runtime::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.contains(&expected_stdout) {
                if let Some(ready) = ready_stdout.lock().take() {
                    let _ = ready.send(Ok(()));
                }
            }
        }
    });
    let ready_stderr = ready.clone();
    let stderr_output = output.clone();
    let stderr_reader = crate::async_runtime::spawn(async move {
        let mut stderr = stderr;
        let mut bytes = [0u8; 4096];
        while let Ok(count) = stderr.read(&mut bytes).await {
            if count == 0 {
                break;
            }
            let listening = {
                let mut output = stderr_output.lock();
                output.append_stderr(&bytes[..count]);
                output.listening(&expected)
            };
            if listening {
                if let Some(ready) = ready_stderr.lock().take() {
                    let _ = ready.send(Ok(()));
                }
            }
        }
    });
    let (stop, mut stopping) = watch::channel(false);
    let (died, death) = watch::channel(false);
    let token_file = token_file.to_owned();
    let marker_path = marker_file.to_owned();
    let supervisor_output = output.clone();
    let process_tab = tab.to_owned();
    #[cfg(unix)]
    let mut exited = tokio::task::spawn_blocking(move || {
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        observe_exit(|| {
            let result =
                unsafe { libc::waitid(libc::P_PID, pid, &mut info, libc::WEXITED | libc::WNOWAIT) };
            if result == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        })
    });
    crate::async_runtime::spawn(async move {
        #[cfg(unix)]
        let status = {
            let group_may_exit = tokio::select! {
                _ = stopping.changed() => true,
                result = &mut exited => match result {
                    Ok(Ok(())) => true,
                    error => {
                        log::debug!("[codex-rpc-process] pid={pid} exit_observer_error={error:?}");
                        false
                    }
                },
            };
            if group_may_exit {
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
                child.wait().await
            } else {
                tokio::select! {
                    _ = stopping.changed() => {
                        unsafe { libc::kill(-(pid as i32), libc::SIGKILL); }
                        child.wait().await
                    },
                    status = child.wait() => status,
                }
            }
        };
        #[cfg(windows)]
        let status = tokio::select! {
            _ = stopping.changed() => {
                job.terminate();
                let _ = child.kill().await;
                child.wait().await
            },
            status = child.wait() => status,
        };
        supervisor_output.lock().exit = Some(match status {
            Ok(status) => status.to_string(),
            Err(error) => format!("wait failed: {error}"),
        });
        let _ = tokio::time::timeout(Duration::from_secs(1), stderr_reader).await;
        if !*stopping.borrow() {
            log::warn!(
                "[codex-rpc-process] tab={process_tab} {}",
                supervisor_output.lock().failure("Sidecar exited", pid)
            );
        }
        if let Some(ready) = ready.lock().take() {
            let _ = ready.send(Err("Sidecar exited before listening"));
        }
        #[cfg(windows)]
        drop(job);
        #[cfg(unix)]
        drop(lease);
        let _ = std::fs::remove_file(token_file);
        let _ = std::fs::remove_file(marker_path);
        died.send_replace(true);
    });
    let process = Process { stop, death };
    let ready = tokio::time::timeout(Duration::from_secs(15), readiness)
        .await
        .map_err(|_| "Sidecar listening deadline")
        .and_then(|result| result.map_err(|_| "Sidecar readiness channel closed"))
        .and_then(|result| result);
    if let Err(error) = ready {
        process.cleanup().finish().await?;
        return Err(output.lock().failure(error, pid));
    }
    let registered = (|| -> Result<(), String> {
        let identity = crate::service::process_tree::launch_processes(pid)
            .into_iter()
            .find(|process| process.pid == pid)
            .ok_or("Sidecar process identity unavailable")?;
        let marker = Marker {
            owner_pid: std::process::id(),
            owner_start_token: crate::service::process_tree::launch_processes(std::process::id())
                .first()
                .map(|process| process.start_token.clone())
                .ok_or("Daemon process identity unavailable")?,
            instance: crate::paths::codeterm_instance_id().into(),
            tab: tab.into(),
            pid,
            start_token: identity.start_token,
            binary: binary.into(),
        };
        std::fs::write(
            marker_file,
            serde_json::to_vec(&marker).map_err(|error| error.to_string())?,
        )
        .map_err(|error| format!("Sidecar marker write {marker_file:?}: {error}"))?;
        Ok(())
    })();
    if let Err(error) = registered {
        process.cleanup().finish().await?;
        return Err(output.lock().failure(&error, pid));
    }
    Ok(process)
}

pub fn sweep(root: &Path, instance: &str) -> Vec<String> {
    let mut errors = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return errors;
    };
    for entry in entries.flatten() {
        if entry.path().extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let marker = std::fs::read(entry.path())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Marker>(&bytes).ok());
        let Some(marker) = marker else { continue };
        if marker.instance != instance {
            continue;
        }
        if marker.owner_pid == std::process::id()
            && crate::service::process_tree::launch_processes(marker.owner_pid)
                .first()
                .is_some_and(|process| process.start_token == marker.owner_start_token)
        {
            continue;
        }
        if identity_matches(&marker, instance) {
            #[cfg(unix)]
            let killed = unsafe {
                libc::getpgid(marker.pid as i32) == marker.pid as i32
                    && libc::kill(-(marker.pid as i32), libc::SIGKILL) == 0
            };
            #[cfg(windows)]
            let killed =
                crate::service::windows_job::terminate_orphan(marker.pid, &marker.start_token);
            if !killed {
                errors.push(format!("Could not terminate owned sidecar {}", marker.tab));
                continue;
            }
        }
        let _ = std::fs::remove_file(entry.path());
        let _ = std::fs::remove_file(entry.path().with_extension("token"));
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn codex_rpc_process_exit_observer_interrupt_is_not_exit() {
        let mut calls = 0;
        observe_exit(|| {
            calls += 1;
            if calls == 1 {
                Err(std::io::Error::from_raw_os_error(libc::EINTR))
            } else {
                Ok(())
            }
        })
        .unwrap();
        assert_eq!(calls, 2);
        let error =
            observe_exit(|| Err(std::io::Error::from_raw_os_error(libc::ECHILD))).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(libc::ECHILD));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn codex_rpc_process_startup_exit_retains_status_and_stderr() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("fake-provider");
        std::fs::write(
            &binary,
            "#!/bin/sh\nprintf 'fixture provider refusal' >&2\nexit 23\n",
        )
        .unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let token = dir.path().join("token");
        let marker = dir.path().join("marker.json");
        std::fs::write(&token, "opaque").unwrap();
        let result = spawn(
            &binary,
            dir.path().to_str().unwrap(),
            Default::default(),
            "fake-tab",
            1,
            &token,
            &marker,
        )
        .await;
        let Err(error) = result else {
            panic!("provider must fail before readiness")
        };
        assert!(
            error.starts_with("Sidecar exited before listening:"),
            "{error}"
        );
        assert!(error.contains("exit=exit status: 23"), "{error}");
        assert!(error.contains("fixture provider refusal"), "{error}");
        assert!(!token.exists());
        assert!(!marker.exists());
    }

    #[tokio::test]
    async fn codex_rpc_process_empty_cwd_error_names_exact_invalid_path() {
        let result = spawn(
            Path::new("unused-binary"),
            "",
            Default::default(),
            "unused-tab",
            1,
            Path::new("unused-token"),
            Path::new("unused-marker"),
        )
        .await;
        let Err(error) = result else {
            panic!("empty cwd must fail before process creation");
        };
        assert!(error.starts_with("Sidecar cwd \"\": "), "{error}");
    }

    #[test]
    fn codex_rpc_process_sweep_leaves_foreign_instance_artifacts() {
        let root = tempfile::tempdir().unwrap();
        let marker = Marker {
            owner_pid: 0,
            owner_start_token: "old".into(),
            instance: "foreign".into(),
            tab: "tab".into(),
            pid: 0,
            start_token: "old".into(),
            binary: PathBuf::from("binary"),
        };
        let path = root.path().join("foreign.json");
        std::fs::write(&path, serde_json::to_vec(&marker).unwrap()).unwrap();
        std::fs::write(path.with_extension("token"), "opaque").unwrap();
        assert!(sweep(root.path(), "own").is_empty());
        assert!(path.exists());
        assert!(path.with_extension("token").exists());
        assert!(!identity_matches(&marker, "own"));
    }

    #[test]
    fn codex_rpc_process_scrubs_provider_identity_through_shared_child_env() {
        let mut command = std::process::Command::new("unused-binary");
        crate::process_util::no_window_std(&mut command);
        command.envs([
            ("CODEX_THREAD_ID", "parent-thread"),
            ("CODEX_SESSION_ID", "parent-session"),
            ("CODEX_CI", "1"),
            ("codex_thread_id", "case-variant"),
            ("CODEX_HOME", "account-home"),
            ("OPENAI_API_KEY", "opaque-credential"),
            ("CODETERM_CODEX_RPC_TOKEN", "own-token"),
            ("CODETERM_SESSION_BINDING_NONCE", "own-nonce"),
        ]);
        let effective = effective_env(&command);
        for (key, value) in codeterm_pty::pty::child_env::child_env_changes_with_exclusions(
            effective
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
            INHERITED_PROVIDER_IDENTITY,
        ) {
            match value {
                Some(value) => command.env(key, value),
                None => command.env_remove(key),
            };
        }
        let effective: std::collections::HashMap<_, _> =
            effective_env(&command).into_iter().collect();
        assert!(!effective.keys().any(|key| INHERITED_PROVIDER_IDENTITY
            .iter()
            .any(|excluded| key.eq_ignore_ascii_case(excluded))));
        assert_eq!(effective["CODEX_HOME"], "account-home");
        assert_eq!(effective["OPENAI_API_KEY"], "opaque-credential");
        assert_eq!(effective["CODETERM_CODEX_RPC_TOKEN"], "own-token");
        assert_eq!(effective["CODETERM_SESSION_BINDING_NONCE"], "own-nonce");
    }

    #[tokio::test]
    async fn codex_rpc_process_cleanup_waits_for_exit_evidence() {
        use futures_util::FutureExt;
        let (stop, mut stopping) = watch::channel(false);
        let (died, death) = watch::channel(false);
        let cleanup = Cleanup { stop, death }.finish();
        tokio::pin!(cleanup);
        assert!(cleanup.as_mut().now_or_never().is_none());
        stopping.changed().await.unwrap();
        assert!(*stopping.borrow());
        died.send_replace(true);
        assert!(cleanup.await.is_ok());
    }
    #[test]
    fn codex_rpc_process_sweep_removes_stale_owned_artifacts() {
        let root = tempfile::tempdir().unwrap();
        let marker = Marker {
            owner_pid: 0,
            owner_start_token: "old".into(),
            instance: "own".into(),
            tab: "tab".into(),
            pid: 0,
            start_token: "old".into(),
            binary: PathBuf::from("binary"),
        };
        let path = root.path().join("owned.json");
        std::fs::write(&path, serde_json::to_vec(&marker).unwrap()).unwrap();
        std::fs::write(path.with_extension("token"), "opaque").unwrap();
        assert!(sweep(root.path(), "own").is_empty());
        assert!(!path.exists());
        assert!(!path.with_extension("token").exists());
    }
}
