use codeterm_repowatch::{RepoEventSink, RepoWatchUnavailable};

pub struct TauriRepoEventSink {
    ui: std::sync::Arc<dyn crate::ports::host::UiEvents>,
}

impl TauriRepoEventSink {
    pub fn new(ui: std::sync::Arc<dyn crate::ports::host::UiEvents>) -> Self {
        Self { ui }
    }
}

impl RepoEventSink for TauriRepoEventSink {
    fn repository_changed(&self, identity: &str) {
        self.ui.emit(
            "repository-changed",
            serde_json::json!({ "identity": identity }),
        );
    }

    fn repository_unavailable(&self, identity: &str, reason: RepoWatchUnavailable) {
        log::warn!("[repowatch] {identity} unavailable: {}", reason.as_str());
        self.ui.emit(
            "repository-unavailable",
            serde_json::json!({ "identity": identity, "reason": reason.as_str() }),
        );
    }
}
