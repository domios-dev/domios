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
use tokio::sync::watch;

pub mod observation;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg(windows)]
fn no_window_tokio(cmd: &mut Command) {
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
#[inline]
fn no_window_tokio(_cmd: &mut Command) {}

const WORKER_TIMEOUT: Duration = Duration::from_secs(300);
/// Narrow no-sleep exception: ETXTBSY is kernel-error recovery, not readiness
/// polling. 10 spawn attempts, 9 backoffs of 20 ms between them (180 ms).
const SPAWN_BUSY_ATTEMPTS: usize = 10;
const SPAWN_BUSY_BACKOFF: Duration = Duration::from_millis(20);
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
pub const KIND_DREAMING_NORMALIZE: &str = "dreaming-normalize";
pub const KIND_LIBRARIAN_ONCE: &str = "librarian-once";
pub const DB_PATH_FLAG: &str = "--db-path";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum WorkerKind {
    DreamingLight,
    DreamingDeep,
    DreamingNormalize,
    LibrarianOnce,
}

impl WorkerKind {
    pub fn as_arg(&self) -> &'static str {
        match self {
            Self::DreamingLight => KIND_DREAMING_LIGHT,
            Self::DreamingDeep => KIND_DREAMING_DEEP,
            Self::DreamingNormalize => KIND_DREAMING_NORMALIZE,
            Self::LibrarianOnce => KIND_LIBRARIAN_ONCE,
        }
    }

    pub fn all() -> [WorkerKind; 4] {
        [
            Self::DreamingLight,
            Self::DreamingDeep,
            Self::DreamingNormalize,
            Self::LibrarianOnce,
        ]
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
    manager: Option<Arc<crate::MemManager>>,
    spawn_busy_retries: watch::Sender<u64>,
    observer: std::sync::OnceLock<std::sync::mpsc::SyncSender<observation::WorkerSpawn>>,
}

impl Supervisor {
    pub fn new(cli_path: PathBuf, db_path: PathBuf) -> Self {
        Self {
            state: Mutex::new(HashMap::new()),
            cli_path,
            db_path,
            manager: None,
            spawn_busy_retries: watch::channel(0).0,
            observer: std::sync::OnceLock::new(),
        }
    }

    pub fn with_manager(cli_path: PathBuf, manager: Arc<crate::MemManager>) -> Self {
        let mut supervisor = Self::new(cli_path, manager.db_path().to_path_buf());
        supervisor.manager = Some(manager);
        supervisor
    }

    fn maintenance_mode(&self) -> crate::backend::engines::MaintenanceMode {
        self.manager.as_ref().map_or_else(
            || crate::backend::engines::MaintenanceMode::Subprocess {
                db_path: self.db_path.clone(),
            },
            |manager| manager.store.maintenance_mode(),
        )
    }

    async fn run_in_process(&self, kind: WorkerKind) -> Result<serde_json::Value, String> {
        let manager = self
            .manager
            .clone()
            .ok_or("maintenance backend is unavailable")?;
        let store = manager.store.background_store()?;
        let worker = tokio::task::spawn_blocking(move || {
            let ctx = crate::backend::WriteContext::system("dreaming:rem");
            if kind == WorkerKind::LibrarianOnce {
                let folders = store.sync_skill_folders(&ctx, &manager.skill_dir);
                let files = crate::file_sync::sync_all_sync(store.as_ref())?;
                Ok(serde_json::json!({
                    "skill_folders_registered":folders.registered.len(),"skill_folders_merged":folders.merged,
                    "skill_folders_tombstoned":folders.skipped_tombstoned.len(),"files_re_indexed":files.re_indexed,
                    "files_unchanged":files.unchanged,"source_missing":files.source_missing,"source_recovered":files.source_recovered,
                    "errors":folders.errors.into_iter().chain(files.errors).collect::<Vec<_>>()
                }))
            } else {
                let machine = manager
                    .skill_dir
                    .parent()
                    .and_then(|root| std::fs::read(root.join("mesh/identity.json")).ok())
                    .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                    .and_then(|value| {
                        value
                            .get("instance_id")
                            .and_then(|id| id.as_str())
                            .map(str::to_owned)
                    })
                    .unwrap_or_else(|| "unknown".into());
                store.dream_worker(&ctx, kind.as_arg(), &machine, WORKER_TIMEOUT)
            }
        });
        tokio::time::timeout(WORKER_TIMEOUT, worker)
            .await
            .map_err(|_| format!("maintenance timeout after {}s", WORKER_TIMEOUT.as_secs()))?
            .map_err(|e| format!("maintenance worker: {e}"))?
    }

    pub fn observe_spawns(
        &self,
        observer: std::sync::mpsc::SyncSender<observation::WorkerSpawn>,
    ) -> bool {
        self.observer.set(observer).is_ok()
    }

    /// Observable count of ETXTBSY spawn refusals, so a caller can wait on the
    /// refusal itself instead of guessing when the writer's descriptor closes.
    pub fn spawn_busy_retries(&self) -> watch::Receiver<u64> {
        self.spawn_busy_retries.subscribe()
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
            return Err(format!(
                "circuit open: {:?} disabled after repeated failures",
                kind
            ));
        }

        let outcome = match self.maintenance_mode() {
            crate::backend::engines::MaintenanceMode::Subprocess { db_path } => {
                self.spawn_and_wait_path(kind, &db_path).await
            }
            crate::backend::engines::MaintenanceMode::InProcess => self.run_in_process(kind).await,
        };
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
                kind,
                entry.recent_failures.len(),
                FAILURE_WINDOW.as_secs(),
            );
        }
    }

    async fn spawn_and_wait_path(
        &self,
        kind: WorkerKind,
        db_path: &std::path::Path,
    ) -> Result<serde_json::Value, String> {
        let mut attempts = 1usize;
        let output = loop {
            let mut cmd = Command::new(&self.cli_path);
            cmd.args([
                MEM_PARENT_SUBCMD,
                RUN_WORKER_SUBCMD,
                kind.as_arg(),
                DB_PATH_FLAG,
                &db_path.to_string_lossy(),
            ]);
            cmd.kill_on_drop(true);
            no_window_tokio(&mut cmd);
            cmd.stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            let child = cmd.spawn();
            if let (Ok(child), Some(observer)) = (&child, self.observer.get()) {
                observation::publish(observer, child, &self.cli_path);
            }
            let output = async { child?.wait_with_output().await };
            match tokio::time::timeout(WORKER_TIMEOUT, output).await {
                Ok(Ok(out)) => break out,
                Ok(Err(e))
                    if e.kind() == std::io::ErrorKind::ExecutableFileBusy
                        && attempts < SPAWN_BUSY_ATTEMPTS =>
                {
                    attempts += 1;
                    self.spawn_busy_retries.send_modify(|n| *n += 1);
                    tokio::time::sleep(SPAWN_BUSY_BACKOFF).await;
                }
                Ok(Err(e)) => return Err(format!("spawn {}: {}", self.cli_path.display(), e)),
                Err(_) => {
                    return Err(format!(
                        "timeout after {}s (cli={})",
                        WORKER_TIMEOUT.as_secs(),
                        self.cli_path.display(),
                    ))
                }
            }
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
            let err = parsed
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("(no error message)");
            return Err(format!("worker reported failure: {}", err));
        }
        Ok(parsed
            .get("report")
            .cloned()
            .unwrap_or(serde_json::Value::Null))
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
mod tests;
