use codeterm_repowatch::{RepoEventSink, RepoWatchUnavailable};

pub type HeadListener = std::sync::Arc<dyn Fn(&str) + Send + Sync>;

pub struct TauriRepoEventSink {
    ui: std::sync::Arc<dyn crate::ports::host::UiEvents>,
    head_listener: parking_lot::RwLock<Option<HeadListener>>,
}

impl TauriRepoEventSink {
    pub fn new(ui: std::sync::Arc<dyn crate::ports::host::UiEvents>) -> Self {
        Self {
            ui,
            head_listener: parking_lot::RwLock::new(None),
        }
    }

    /// The service is built after the registry, so it subscribes here once constructed.
    pub fn bind_head_listener(&self, listener: HeadListener) {
        *self.head_listener.write() = Some(listener);
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

    fn repository_head_moved(&self, identity: &str) {
        let listener = self.head_listener.read().clone();
        if let Some(listener) = listener {
            listener(identity);
        }
    }
}
