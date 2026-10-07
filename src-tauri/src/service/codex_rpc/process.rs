use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{oneshot, watch};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Marker {
    pub owner_pid: u32,
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
    pub marker: Marker,
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
    let mut command = tokio::process::Command::new(binary);
    command
        .args([
            "app-server",
            "--listen",
            &format!("ws://127.0.0.1:{port}"),
            "--ws-auth",
            "capability-token",
            "--ws-token-file",
        ])
        .arg(token_file)
        .current_dir(cwd)
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(windows)]
    command.creation_flags(0x08000000 | 0x00000004);
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    let pid = child.id().ok_or("Sidecar has no process id")?;
    #[cfg(windows)]
    let job = super::windows_job::Job::attach(&child)?;
    let identity = crate::service::process_tree::launch_processes(pid)
        .into_iter()
        .find(|process| process.pid == pid)
        .ok_or("Sidecar process identity unavailable")?;
    let marker = Marker {
        owner_pid: std::process::id(),
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
    .map_err(|error| error.to_string())?;
    let (ready, readiness) = oneshot::channel();
    let ready = std::sync::Arc::new(parking_lot::Mutex::new(Some(ready)));
    let expected = format!("listening on: ws://127.0.0.1:{port}");
    let stdout = child.stdout.take().ok_or("Missing sidecar stdout")?;
    let stderr = child.stderr.take().ok_or("Missing sidecar stderr")?;
    let ready_stdout = ready.clone();
    let expected_stdout = expected.clone();
    tauri::async_runtime::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.contains(&expected_stdout) {
                if let Some(ready) = ready_stdout.lock().take() {
                    let _ = ready.send(());
                }
            }
        }
    });
    tauri::async_runtime::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.contains(&expected) {
                if let Some(ready) = ready.lock().take() {
                    let _ = ready.send(());
                }
            }
        }
    });
    let (stop, mut stopping) = watch::channel(false);
    let (died, death) = watch::channel(false);
    let token_file = token_file.to_owned();
    let marker_file = marker_file.to_owned();
    #[cfg(unix)]
    let mut exited = tokio::task::spawn_blocking(move || {
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        unsafe { libc::waitid(libc::P_PID, pid, &mut info, libc::WEXITED | libc::WNOWAIT) }
    });
    tauri::async_runtime::spawn(async move {
        #[cfg(unix)]
        {
            tokio::select! { _ = stopping.changed() => {}, _ = &mut exited => {} }
            unsafe { libc::kill(-(pid as i32), libc::SIGKILL); }
            let _ = child.wait().await;
        }
        #[cfg(windows)]
        tokio::select! {
            _ = stopping.changed() => {
                #[cfg(unix)]
                unsafe { libc::kill(-(pid as i32), libc::SIGKILL); }
                #[cfg(windows)]
                job.terminate();
                let _ = child.kill().await;
                let _ = child.wait().await;
            },
            _ = child.wait() => {}
        }
        #[cfg(windows)]
        drop(job);
        died.send_replace(true);
        let _ = std::fs::remove_file(token_file);
        let _ = std::fs::remove_file(marker_file);
    });
    let process = Process {
        stop,
        death,
        marker,
    };
    tokio::time::timeout(Duration::from_secs(15), readiness)
        .await
        .map_err(|_| "Sidecar listening deadline")?
        .map_err(|_| "Sidecar exited before listening")?;
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
        if marker.owner_pid == std::process::id() {
            continue;
        }
        if identity_matches(&marker, instance) {
            #[cfg(unix)]
            let killed = unsafe { libc::kill(-(marker.pid as i32), libc::SIGKILL) == 0 };
            #[cfg(windows)]
            let killed = super::windows_job::terminate_orphan(marker.pid, &marker.start_token);
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
