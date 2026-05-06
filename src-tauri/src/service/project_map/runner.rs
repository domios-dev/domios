use parking_lot::Mutex;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, serde::Serialize)]
pub struct RunSummary {
    pub workspace_id: String,
    pub ok: bool,
    pub message: String,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone, serde::Serialize, Default)]
pub struct IndexProgress {
    pub running: bool,
    pub phase: String,
    pub done: usize,
    pub total: usize,
    pub started_at_ms: i64,
}

pub struct Runner {
    pub cli_path: PathBuf,
    pub socket_path: String,
}

impl Runner {
    pub fn new(cli_path: PathBuf, socket_path: String) -> Runner {
        Runner { cli_path, socket_path }
    }

    pub fn run_index(&self, workspace_id: &str, force: bool, progress: Arc<Mutex<IndexProgress>>) -> RunSummary {
        let start = std::time::Instant::now();
        let mut cmd = std::process::Command::new(&self.cli_path);
        cmd.arg("--json")
            .arg("map")
            .arg("index")
            .arg("--workspace")
            .arg(workspace_id)
            .arg("--progress");
        if force {
            cmd.arg("--full");
        }
        cmd.env("CODETERM_SOCKET_PATH", &self.socket_path);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());

        {
            let mut p = progress.lock();
            *p = IndexProgress {
                running: true,
                phase: "scan".into(),
                done: 0,
                total: 0,
                started_at_ms: chrono::Utc::now().timestamp_millis(),
            };
        }

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                progress.lock().running = false;
                return RunSummary {
                    workspace_id: workspace_id.to_string(),
                    ok: false,
                    message: format!("spawn failed: {}", e),
                    elapsed_ms: start.elapsed().as_millis(),
                };
            }
        };
        let stderr = child.stderr.take();
        let stderr_progress = progress.clone();
        let stderr_handle = stderr.map(|s| {
            std::thread::spawn(move || {
                let reader = BufReader::new(s);
                let mut captured = String::new();
                for line in reader.lines().flatten() {
                    if let Some(rest) = line.strip_prefix("__progress__ ") {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(rest) {
                            let mut p = stderr_progress.lock();
                            p.phase = v.get("phase").and_then(|x| x.as_str()).unwrap_or("").to_string();
                            p.done = v.get("done").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
                            p.total = v.get("total").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
                        }
                    } else {
                        captured.push_str(&line);
                        captured.push('\n');
                    }
                }
                captured
            })
        });

        let mut stdout_buf = String::new();
        if let Some(mut s) = child.stdout.take() {
            use std::io::Read;
            let _ = s.read_to_string(&mut stdout_buf);
        }
        let status = match child.wait() {
            Ok(s) => s,
            Err(e) => {
                progress.lock().running = false;
                return RunSummary {
                    workspace_id: workspace_id.to_string(),
                    ok: false,
                    message: format!("wait failed: {}", e),
                    elapsed_ms: start.elapsed().as_millis(),
                };
            }
        };
        let captured_err = stderr_handle.and_then(|h| h.join().ok()).unwrap_or_default();
        progress.lock().running = false;
        let elapsed_ms = start.elapsed().as_millis();
        if status.success() {
            RunSummary {
                workspace_id: workspace_id.to_string(),
                ok: true,
                message: stdout_buf.trim().to_string(),
                elapsed_ms,
            }
        } else {
            RunSummary {
                workspace_id: workspace_id.to_string(),
                ok: false,
                message: format!(
                    "exit {}: {}",
                    status.code().unwrap_or(-1),
                    captured_err.trim()
                ),
                elapsed_ms,
            }
        }
    }
}

/// Coordinated re-run guard: while one index is running for a workspace,
/// the next request becomes a "pending" flag. After the running one finishes
/// we re-check the flag and dispatch one more run if it was set.
pub struct RunCoordinator {
    pub in_flight: Arc<AtomicBool>,
    pub pending: Arc<AtomicBool>,
}

impl RunCoordinator {
    pub fn new() -> Self {
        Self {
            in_flight: Arc::new(AtomicBool::new(false)),
            pending: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Returns true if the caller should proceed to dispatch the run.
    /// Returns false if a run is already in flight (the pending flag is set).
    pub fn try_acquire(&self) -> bool {
        if self.in_flight.swap(true, Ordering::AcqRel) {
            self.pending.store(true, Ordering::Release);
            false
        } else {
            true
        }
    }

    /// Returns true if the caller should immediately dispatch another run.
    pub fn release(&self) -> bool {
        self.in_flight.store(false, Ordering::Release);
        self.pending.swap(false, Ordering::AcqRel)
    }
}

/// Best-effort path to the codeterm CLI. Mirrors `Supervisor::resolve_cli_path`.
pub fn resolve_cli_path(data_dir: &Path) -> PathBuf {
    crate::mem::supervisor::Supervisor::resolve_cli_path(data_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinator_basic_acquire_release() {
        let c = RunCoordinator::new();
        assert!(c.try_acquire(), "first acquire ok");
        assert!(!c.try_acquire(), "second acquire while in flight: rejected");
        assert!(c.release(), "release returns true because pending was set");
        assert!(c.try_acquire(), "after release, acquire works again");
        assert!(!c.release(), "no pending second time");
    }

    #[test]
    fn run_index_missing_binary_errors() {
        let runner = Runner::new(
            PathBuf::from("/no/such/binary/codeterm-test-stub"),
            "test-socket".to_string(),
        );
        let progress = Arc::new(Mutex::new(IndexProgress::default()));
        let summary = runner.run_index("ws-1", false, progress);
        assert!(!summary.ok);
        assert!(summary.message.contains("spawn failed"));
    }
}
