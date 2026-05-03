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

/// CLI subcommand wiring. Both halves (daemon-side spawn, cli-side handler)
/// MUST stay in sync. `cli_source_still_declares_run_worker` below pins the
/// CLI source via `include_str!`, and `cli/src/main.rs::subcommand_contract_tests`
/// pins the clap parser. Drift breaks `cargo test --lib` immediately.
pub const MEM_PARENT_SUBCMD: &str = "mem";
pub const RUN_WORKER_SUBCMD: &str = "run-worker";
pub const KIND_DREAMING_LIGHT: &str = "dreaming-light";
pub const KIND_DREAMING_DEEP: &str = "dreaming-deep";
pub const KIND_LIBRARIAN_ONCE: &str = "librarian-once";
pub const DB_PATH_FLAG: &str = "--db-path";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum WorkerKind {
    DreamingLight,
    DreamingDeep,
    LibrarianOnce,
}

impl WorkerKind {
    pub fn as_arg(&self) -> &'static str {
        match self {
            Self::DreamingLight => KIND_DREAMING_LIGHT,
            Self::DreamingDeep => KIND_DREAMING_DEEP,
            Self::LibrarianOnce => KIND_LIBRARIAN_ONCE,
        }
    }

    pub fn all() -> [WorkerKind; 3] {
        [Self::DreamingLight, Self::DreamingDeep, Self::LibrarianOnce]
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
    /// Resolution order:
    ///   1. `CODETERM_CLI_PATH` env override, if it points to an existing file.
    ///   2. `<data_dir>/bin/codeterm[.exe]` (installed via post-build copy).
    ///   3. `current_exe()` itself when the daemon exe IS the CLI (Windows MSI / single-binary install).
    ///   4. Sibling of `current_exe()` named `codeterm[.exe]` (covers macOS .app bundle and dev `target/`).
    ///   5. `codeterm[.exe]` discovered on `PATH`.
    ///   6. Fallback: the (possibly non-existent) installed path so the spawn error is loggable.
    pub fn resolve_cli_path(data_dir: &std::path::Path) -> PathBuf {
        resolve_cli_path_inner(
            data_dir,
            std::env::var_os("CODETERM_CLI_PATH").map(PathBuf::from),
            std::env::current_exe().ok(),
            std::env::var_os("PATH"),
        )
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
            MEM_PARENT_SUBCMD,
            RUN_WORKER_SUBCMD,
            kind.as_arg(),
            DB_PATH_FLAG,
            &self.db_path.to_string_lossy(),
        ]);
        cmd.kill_on_drop(true);

        let output = match tokio::time::timeout(WORKER_TIMEOUT, cmd.output()).await {
            Ok(Ok(out)) => out,
            Ok(Err(e)) => return Err(format!("spawn {}: {}", self.cli_path.display(), e)),
            Err(_) => return Err(format!(
                "timeout after {}s (cli={})",
                WORKER_TIMEOUT.as_secs(),
                self.cli_path.display(),
            )),
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(format!(
                "exit {} (cli={}): stderr={} stdout={}",
                output.status.code().unwrap_or(-1),
                self.cli_path.display(),
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

fn resolve_cli_path_inner(
    data_dir: &std::path::Path,
    env_override: Option<PathBuf>,
    current_exe: Option<PathBuf>,
    path_env: Option<std::ffi::OsString>,
) -> PathBuf {
    let exe_names: &[&str] = if cfg!(windows) {
        &["codeterm.exe"]
    } else {
        &["codeterm"]
    };
    let primary = exe_names[0];

    if let Some(p) = env_override {
        if p.is_file() {
            return p;
        }
    }

    let installed = data_dir.join("bin").join(primary);
    if installed.is_file() {
        return installed;
    }

    if let Some(current) = current_exe.as_ref() {
        if is_cli_binary(current, exe_names) {
            return current.clone();
        }
        if let Some(parent) = current.parent() {
            for name in exe_names {
                let sibling = parent.join(name);
                if sibling.is_file() && sibling.as_path() != current.as_path() {
                    return sibling;
                }
            }
        }
    }

    if let Some(paths) = path_env {
        for dir in std::env::split_paths(&paths) {
            for name in exe_names {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
    }

    installed
}

fn is_cli_binary(path: &std::path::Path, exe_names: &[&str]) -> bool {
    let Some(file_name) = path.file_name().and_then(|f| f.to_str()) else {
        return false;
    };
    if !exe_names.iter().any(|n| file_name.eq_ignore_ascii_case(n)) {
        return false;
    }
    let s = path.to_string_lossy();
    !s.contains(".app/") && !s.contains("\\target\\") && !s.contains("/target/")
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

    fn cli_filename() -> &'static str {
        if cfg!(windows) { "codeterm.exe" } else { "codeterm" }
    }

    fn touch(path: &std::path::Path) {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).unwrap();
        }
        std::fs::write(path, b"").unwrap();
    }

    #[test]
    fn resolve_cli_path_env_override_wins() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("data");
        std::fs::create_dir_all(data_dir.join("bin")).unwrap();
        let installed = data_dir.join("bin").join(cli_filename());
        touch(&installed);

        let override_path = dir.path().join("custom").join(cli_filename());
        touch(&override_path);

        let resolved = resolve_cli_path_inner(
            &data_dir,
            Some(override_path.clone()),
            None,
            None,
        );
        assert_eq!(resolved, override_path, "env override must win over installed");
    }

    #[test]
    fn resolve_cli_path_env_override_missing_falls_through() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("data");
        std::fs::create_dir_all(data_dir.join("bin")).unwrap();
        let installed = data_dir.join("bin").join(cli_filename());
        touch(&installed);

        let bogus = dir.path().join("does-not-exist").join(cli_filename());
        let resolved = resolve_cli_path_inner(&data_dir, Some(bogus), None, None);
        assert_eq!(resolved, installed, "missing override should fall through to installed");
    }

    #[test]
    fn resolve_cli_path_app_sibling_found() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("data");
        let app = dir.path().join("MyApp.app").join("Contents").join("MacOS");
        std::fs::create_dir_all(&app).unwrap();
        let daemon = app.join("daemon-bin");
        touch(&daemon);
        let cli_sibling = app.join(cli_filename());
        touch(&cli_sibling);

        let resolved = resolve_cli_path_inner(&data_dir, None, Some(daemon), None);
        assert_eq!(resolved, cli_sibling);
    }

    #[test]
    fn resolve_cli_path_target_sibling_found() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("data");
        let target_dir = dir.path().join("target").join("debug");
        std::fs::create_dir_all(&target_dir).unwrap();
        let daemon = target_dir.join("some-daemon");
        touch(&daemon);
        let cli_sibling = target_dir.join(cli_filename());
        touch(&cli_sibling);

        let resolved = resolve_cli_path_inner(&data_dir, None, Some(daemon), None);
        assert_eq!(resolved, cli_sibling);
    }

    #[test]
    fn resolve_cli_path_current_exe_is_cli_when_outside_target() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("data");
        let install_dir = dir.path().join("Program Files").join("CodeTerm");
        std::fs::create_dir_all(&install_dir).unwrap();
        let exe = install_dir.join(cli_filename());
        touch(&exe);

        let resolved = resolve_cli_path_inner(&data_dir, None, Some(exe.clone()), None);
        assert_eq!(resolved, exe);
    }

    #[test]
    fn resolve_cli_path_path_env_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("data");
        let path_dir = dir.path().join("bin-on-path");
        std::fs::create_dir_all(&path_dir).unwrap();
        let on_path = path_dir.join(cli_filename());
        touch(&on_path);

        let path_env = std::env::join_paths([&path_dir]).unwrap();
        let resolved = resolve_cli_path_inner(&data_dir, None, None, Some(path_env));
        assert_eq!(resolved, on_path);
    }

    #[test]
    fn resolve_cli_path_none_found_returns_installed() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("data");
        let resolved = resolve_cli_path_inner(&data_dir, None, None, None);
        assert_eq!(resolved, data_dir.join("bin").join(cli_filename()));
    }

    /// Pin the wire contract with the CLI. If any of these strings change here,
    /// `cli/src/main.rs::subcommand_contract_tests` must change in lockstep —
    /// both halves are read together by the developer and grep-able.
    #[test]
    fn cli_subcommand_strings_are_pinned() {
        assert_eq!(MEM_PARENT_SUBCMD, "mem");
        assert_eq!(RUN_WORKER_SUBCMD, "run-worker");
        assert_eq!(KIND_DREAMING_LIGHT, "dreaming-light");
        assert_eq!(KIND_DREAMING_DEEP, "dreaming-deep");
        assert_eq!(KIND_LIBRARIAN_ONCE, "librarian-once");
        assert_eq!(DB_PATH_FLAG, "--db-path");

        assert_eq!(WorkerKind::DreamingLight.as_arg(), KIND_DREAMING_LIGHT);
        assert_eq!(WorkerKind::DreamingDeep.as_arg(), KIND_DREAMING_DEEP);
        assert_eq!(WorkerKind::LibrarianOnce.as_arg(), KIND_LIBRARIAN_ONCE);
    }

    /// Cross-crate sanity check: the CLI source still contains the literal
    /// `#[clap(name = "run-worker")]` attribute and accepts the three kind
    /// strings in its doc comment. clap's `#[clap(name = ...)]` requires a
    /// literal so we can't share a const directly — this string-search keeps
    /// the two halves wired together at `cargo test --lib` time without
    /// needing the CLI binary to be built first.
    #[test]
    fn cli_source_still_declares_run_worker() {
        let cli_src = include_str!("../../../cli/src/main.rs");
        assert!(
            cli_src.contains(r#"#[clap(name = "run-worker")]"#),
            "cli/src/main.rs no longer declares `#[clap(name = \"run-worker\")]` — \
             supervisor.rs spawns this exact subcommand. Fix one or the other."
        );
        for kind in [KIND_DREAMING_LIGHT, KIND_DREAMING_DEEP, KIND_LIBRARIAN_ONCE] {
            assert!(
                cli_src.contains(&format!("\"{}\"", kind)),
                "cli/src/main.rs no longer references kind {:?} — supervisor.rs spawns it.",
                kind
            );
        }
    }
}
