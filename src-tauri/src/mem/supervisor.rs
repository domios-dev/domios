//! Supervisor for out-of-process mem maintenance workers (dreaming, librarian).
//!
//! The daemon spawns the CLI as a subprocess for each maintenance pass so a
//! panic / OOM / hang in dreaming or the librarian cannot take the API down.
//! Per-kind circuit breaker disables a kind after 3 failures within 60 s; the
//! circuit only re-opens on an explicit `reset(kind)` call (no automatic
//! recovery — operator must investigate).

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tokio::process::Command;

const WORKER_TIMEOUT: Duration = Duration::from_secs(300);
const FAILURE_WINDOW: Duration = Duration::from_secs(60);
const FAILURE_THRESHOLD: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum WorkerKind {
    DreamingLight,
    DreamingDeep,
    LibrarianOnce,
}

impl WorkerKind {
    fn as_arg(&self) -> &'static str {
        match self {
            Self::DreamingLight => "dreaming-light",
            Self::DreamingDeep => "dreaming-deep",
            Self::LibrarianOnce => "librarian-once",
        }
    }
}

#[derive(Default)]
struct KindState {
    recent_failures: VecDeque<Instant>,
    disabled: bool,
}

pub struct Supervisor {
    state: Mutex<HashMap<WorkerKind, KindState>>,
    cli_path: PathBuf,
    db_path: PathBuf,
}

impl Supervisor {
    pub fn new(cli_path: PathBuf, db_path: PathBuf) -> Self {
        Self {
            state: Mutex::new(HashMap::new()),
            cli_path,
            db_path,
        }
    }

    /// Resolve the CLI binary path used to spawn maintenance workers.
    /// Prefers the installed CLI under the data dir's `bin/` folder; falls
    /// back to `current_exe()` only when that's clearly the CLI binary
    /// (its filename matches `codeterm`/`codeterm.exe`). Bundled .app and
    /// dev `target/` daemon binaries are skipped because they cannot
    /// handle the `mem run-worker` subcommand.
    pub fn resolve_cli_path(data_dir: &std::path::Path) -> PathBuf {
        let exe_name = if cfg!(windows) { "codeterm.exe" } else { "codeterm" };
        let installed = data_dir.join("bin").join(exe_name);
        if installed.is_file() {
            return installed;
        }
        if let Ok(current) = std::env::current_exe() {
            let s = current.to_string_lossy();
            if !s.contains(".app/") && !s.contains("\\target\\") && !s.contains("/target/") {
                return current;
            }
        }
        installed
    }

    /// Run one maintenance pass for `kind` as a subprocess. Returns the
    /// parsed `report` JSON on success, an error string on failure /
    /// timeout / circuit open. Failures within the 60 s rolling window
    /// open the circuit after 3 strikes — subsequent calls return
    /// "circuit open" until [`reset`] is called.
    pub async fn run(&self, kind: WorkerKind) -> Result<serde_json::Value, String> {
        if self.is_disabled(kind) {
            return Err(format!("circuit open: {:?} disabled after repeated failures", kind));
        }

        let outcome = self.spawn_and_wait(kind).await;
        match &outcome {
            Ok(_) => self.record_success(kind),
            Err(e) => {
                log::warn!("[mem.supervisor] {:?} failed: {}", kind, e);
                self.record_failure(kind);
            }
        }
        outcome
    }

    /// Re-enable a kind after the operator has investigated repeated failures.
    pub fn reset(&self, kind: WorkerKind) {
        let mut state = self.state.lock();
        let entry = state.entry(kind).or_default();
        entry.recent_failures.clear();
        entry.disabled = false;
    }

    fn is_disabled(&self, kind: WorkerKind) -> bool {
        self.state.lock().get(&kind).map_or(false, |s| s.disabled)
    }

    fn record_success(&self, kind: WorkerKind) {
        let mut state = self.state.lock();
        let entry = state.entry(kind).or_default();
        entry.recent_failures.clear();
    }

    fn record_failure(&self, kind: WorkerKind) {
        let mut state = self.state.lock();
        let entry = state.entry(kind).or_default();
        let now = Instant::now();
        entry.recent_failures.push_back(now);
        let cutoff = now - FAILURE_WINDOW;
        while let Some(&front) = entry.recent_failures.front() {
            if front < cutoff {
                entry.recent_failures.pop_front();
            } else {
                break;
            }
        }
        if entry.recent_failures.len() >= FAILURE_THRESHOLD && !entry.disabled {
            entry.disabled = true;
            log::error!(
                "[mem.supervisor] circuit open for {:?} ({} failures within {}s)",
                kind, entry.recent_failures.len(), FAILURE_WINDOW.as_secs(),
            );
        }
    }

    async fn spawn_and_wait(&self, kind: WorkerKind) -> Result<serde_json::Value, String> {
        let mut cmd = Command::new(&self.cli_path);
        cmd.args([
            "mem",
            "run-worker",
            kind.as_arg(),
            "--db-path",
            &self.db_path.to_string_lossy(),
        ]);
        cmd.kill_on_drop(true);

        let output = match tokio::time::timeout(WORKER_TIMEOUT, cmd.output()).await {
            Ok(Ok(out)) => out,
            Ok(Err(e)) => return Err(format!("spawn: {}", e)),
            Err(_) => return Err(format!("timeout after {}s", WORKER_TIMEOUT.as_secs())),
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(format!(
                "exit {}: stderr={} stdout={}",
                output.status.code().unwrap_or(-1),
                stderr.trim(),
                stdout.trim(),
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let parsed: serde_json::Value = serde_json::from_str(stdout.trim())
            .map_err(|e| format!("parse stdout: {} (raw={:?})", e, stdout.trim()))?;
        if !parsed.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
            let err = parsed.get("error").and_then(|v| v.as_str()).unwrap_or("(no error message)");
            return Err(format!("worker reported failure: {}", err));
        }
        Ok(parsed.get("report").cloned().unwrap_or(serde_json::Value::Null))
    }
}

pub type SupervisorState = Arc<Supervisor>;

#[cfg(test)]
mod tests {
    use super::*;

    fn make_supervisor(cli: PathBuf) -> Supervisor {
        Supervisor::new(cli, PathBuf::from(":memory:"))
    }

    /// Write a fake CLI script that echoes `stdout` and exits with `exit_code`.
    /// Returns the path to the script (kept alive by `_dir`).
    #[cfg(unix)]
    fn fake_cli(dir: &tempfile::TempDir, stdout: &str, exit_code: i32) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let script = dir.path().join("fake-cli");
        let body = format!("#!/bin/sh\necho '{}'\nexit {}\n", stdout.replace('\'', "'\\''"), exit_code);
        std::fs::write(&script, body).unwrap();
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
        script
    }

    #[cfg(windows)]
    fn fake_cli(dir: &tempfile::TempDir, stdout: &str, exit_code: i32) -> PathBuf {
        let script = dir.path().join("fake-cli.cmd");
        let body = format!("@echo off\r\necho {}\r\nexit /b {}\r\n", stdout, exit_code);
        std::fs::write(&script, body).unwrap();
        script
    }

    #[tokio::test]
    async fn run_returns_report_on_success() {
        let dir = tempfile::tempdir().unwrap();
        let cli = fake_cli(&dir, r#"{"ok":true,"report":{"phase":"light","considered":3,"promoted":1,"dropped":0}}"#, 0);
        let sup = make_supervisor(cli);
        let report = sup.run(WorkerKind::DreamingLight).await.unwrap();
        assert_eq!(report.get("phase").and_then(|v| v.as_str()), Some("light"));
        assert_eq!(report.get("promoted").and_then(|v| v.as_u64()), Some(1));
    }

    #[tokio::test]
    async fn run_returns_error_when_subprocess_fails() {
        let dir = tempfile::tempdir().unwrap();
        let cli = fake_cli(&dir, r#"{"ok":false,"error":"db locked"}"#, 1);
        let sup = make_supervisor(cli);
        let err = sup.run(WorkerKind::DreamingLight).await.unwrap_err();
        assert!(err.contains("exit 1") || err.contains("db locked"), "got: {}", err);
    }

    #[tokio::test]
    async fn three_failures_open_circuit() {
        let dir = tempfile::tempdir().unwrap();
        let cli = fake_cli(&dir, "boom", 1);
        let sup = make_supervisor(cli);
        for _ in 0..3 {
            let _ = sup.run(WorkerKind::DreamingDeep).await;
        }
        let err = sup.run(WorkerKind::DreamingDeep).await.unwrap_err();
        assert!(err.contains("circuit open"), "expected circuit open, got: {}", err);

        sup.reset(WorkerKind::DreamingDeep);
        let err2 = sup.run(WorkerKind::DreamingDeep).await.unwrap_err();
        assert!(!err2.contains("circuit open"), "circuit should re-open after reset, got: {}", err2);
    }

    #[tokio::test]
    async fn nonexistent_cli_path_fails_cleanly() {
        let sup = make_supervisor(PathBuf::from("/definitely/not/a/real/path/codeterm-mem-worker-nope"));
        let err = sup.run(WorkerKind::LibrarianOnce).await.unwrap_err();
        assert!(err.contains("spawn"), "expected spawn error, got: {}", err);
    }
}
