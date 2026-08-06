//! Failure visibility for updates that abort AFTER destructive teardown.
//! Writes a diagnostic artifact under the `~/.codeterm/crashes/` convention and
//! raises a user-visible failure signal, so a torn-down workspace never fails
//! silently into a buried ERROR log line.

use std::fmt::Write;
use std::path::{Path, PathBuf};

pub const NEXT_STEPS: &str =
    "The update was aborted after panes were torn down. Restart CodeTerm to reload the \
     persisted session snapshot; agent tabs auto-resume by session id. Then retry the update.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateAbort {
    pub phase: String,
    pub cause: String,
    pub torn_down_tabs: Vec<String>,
    pub next_steps: String,
}

impl UpdateAbort {
    pub fn new(phase: &str, cause: &str, torn_down_tabs: Vec<String>) -> Self {
        Self {
            phase: phase.to_string(),
            cause: cause.to_string(),
            torn_down_tabs,
            next_steps: NEXT_STEPS.to_string(),
        }
    }
}

/// The seam every abort routes through: emit whatever the caller wants the user
/// to see. Injected so the orchestration is testable with a spy. `Send + Sync`
/// so a `&dyn UpdateFailureSink` may be held across `.await` in the orchestration
/// future the Tauri command requires to be `Send`.
pub trait UpdateFailureSink: Send + Sync {
    fn abort(&self, abort: &UpdateAbort);
}

pub fn write_abort_artifact(abort: &UpdateAbort) -> Option<PathBuf> {
    write_abort_artifact_in(&crate::crash::report::crashes_dir(), abort)
}

pub(crate) fn write_abort_artifact_in(dir: &Path, abort: &UpdateAbort) -> Option<PathBuf> {
    if let Err(e) = std::fs::create_dir_all(dir) {
        log::error!("[updater] create crashes dir {}: {}", dir.display(), e);
        return None;
    }
    let path = dir.join(format!(
        "crash-{}-update-abort.txt",
        crate::crash::report::timestamp_slug()
    ));
    match std::fs::write(&path, render_abort(abort)) {
        Ok(()) => Some(path),
        Err(e) => {
            log::error!("[updater] write abort artifact {}: {}", path.display(), e);
            None
        }
    }
}

fn render_abort(abort: &UpdateAbort) -> String {
    let mut s = crate::crash::report::header("update-abort");
    let _ = writeln!(s);
    let _ = writeln!(s, "Phase: {}", abort.phase);
    let _ = writeln!(s, "Cause: {}", abort.cause);
    let _ = writeln!(s, "Torn-down tabs: {}", abort.torn_down_tabs.len());
    for tab in &abort.torn_down_tabs {
        let _ = writeln!(s, "  - {}", tab);
    }
    let _ = writeln!(s);
    let _ = writeln!(s, "Next steps:");
    let _ = writeln!(s, "{}", abort.next_steps);
    let _ = writeln!(s);
    let _ = writeln!(s, "Recent log tail (last 200 lines):");
    let _ = writeln!(s, "---");
    let _ = writeln!(s, "{}", crate::crash::report::log_tail(200));
    s
}

/// Production sink: durable artifact + error state (drives the update toast) +
/// dedicated frontend event + OS-level toast that survives an unfocused/torn-down
/// webview.
pub struct AppFailureSink {
    pub app: tauri::AppHandle,
    pub update_state: crate::UpdateStateHandle,
}

impl UpdateFailureSink for AppFailureSink {
    fn abort(&self, abort: &UpdateAbort) {
        let artifact = write_abort_artifact(abort);
        let artifact_str = artifact.as_ref().map(|p| p.display().to_string());

        self.update_state.lock().set_error(abort.cause.clone());

        crate::api::events::emit_everywhere(
            &self.app,
            "update-failed",
            serde_json::json!({
                "phase": abort.phase,
                "cause": abort.cause,
                "torn_down_tabs": abort.torn_down_tabs,
                "artifact": artifact_str,
                "next_steps": abort.next_steps,
            }),
        );

        let body = format!(
            "{} — diagnostic: {}",
            abort.cause,
            artifact_str.as_deref().unwrap_or("unavailable")
        );
        crate::api::os_notify::_send_os_notification(
            &self.app,
            "CodeTerm update failed",
            Some(&body),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_captures_phase_cause_and_torn_down_tabs() {
        let dir = std::env::temp_dir().join(format!("ct-abort-test-{}", std::process::id()));
        let abort = UpdateAbort::new(
            "install",
            "installer launch failed: exit 137",
            vec!["tab-worker-1".to_string(), "tab-worker-2".to_string()],
        );
        let path = write_abort_artifact_in(&dir, &abort).expect("artifact written");
        let body = std::fs::read_to_string(&path).expect("artifact readable");

        assert!(body.contains("Phase: install"));
        assert!(body.contains("installer launch failed: exit 137"));
        assert!(body.contains("Torn-down tabs: 2"));
        assert!(body.contains("tab-worker-1"));
        assert!(body.contains("tab-worker-2"));
        assert!(body.contains(NEXT_STEPS));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn artifact_is_written_under_crashes_convention() {
        let dir = std::env::temp_dir().join(format!("ct-abort-conv-{}", std::process::id()));
        let abort = UpdateAbort::new("shutdown", "PTY reap failed", Vec::new());
        let path = write_abort_artifact_in(&dir, &abort).expect("artifact written");
        let name = path.file_name().and_then(|n| n.to_str()).unwrap();
        assert!(name.starts_with("crash-"));
        assert!(name.ends_with("-update-abort.txt"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
