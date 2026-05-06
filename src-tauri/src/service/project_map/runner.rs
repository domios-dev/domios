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

pub struct Runner {
    pub cli_path: PathBuf,
    pub socket_path: String,
}

impl Runner {
    pub fn new(cli_path: PathBuf, socket_path: String) -> Runner {
        Runner { cli_path, socket_path }
    }

    /// Spawn `codeterm map index --workspace <id> --json` and wait for it.
    /// Designed to be called from a tokio blocking task or a std thread —
    /// it blocks until the subprocess exits.
    pub fn run_index(&self, workspace_id: &str, force: bool) -> RunSummary {
        let start = std::time::Instant::now();
        let mut cmd = std::process::Command::new(&self.cli_path);
        cmd.arg("--json")
            .arg("map")
            .arg("index")
            .arg("--workspace")
            .arg(workspace_id);
        if force {
            cmd.arg("--full");
        }
        cmd.env("CODETERM_SOCKET_PATH", &self.socket_path);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());

        let output = match cmd.output() {
            Ok(o) => o,
            Err(e) => {
                return RunSummary {
                    workspace_id: workspace_id.to_string(),
                    ok: false,
                    message: format!("spawn failed: {}", e),
                    elapsed_ms: start.elapsed().as_millis(),
                };
            }
        };
        let elapsed_ms = start.elapsed().as_millis();
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            RunSummary {
                workspace_id: workspace_id.to_string(),
                ok: true,
                message: stdout.trim().to_string(),
                elapsed_ms,
            }
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            RunSummary {
                workspace_id: workspace_id.to_string(),
                ok: false,
                message: format!(
                    "exit {}: {}",
                    output.status.code().unwrap_or(-1),
                    stderr.trim()
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
        let summary = runner.run_index("ws-1", false);
        assert!(!summary.ok);
        assert!(summary.message.contains("spawn failed"));
    }
}
