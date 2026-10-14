use codeterm_repowatch::{RepoEventSink, RepoWatchUnavailable};

pub struct TauriRepoEventSink {
    app: crate::app_runtime::AppHandle,
}

impl TauriRepoEventSink {
    pub fn new(app: crate::app_runtime::AppHandle) -> Self {
        Self { app }
    }
}

impl RepoEventSink for TauriRepoEventSink {
    fn repository_changed(&self, identity: &str) {
        codeterm_events::emit_everywhere(
            &self.app,
            "repository-changed",
            serde_json::json!({ "identity": identity }),
        );
    }

    fn repository_head_changed(&self, identity: &str) {
        use tauri::Manager;
        if let Some(service) = self.app.try_state::<crate::service::ServiceState>() {
            service
                .inner()
                .offer_merged_lane_cleanup(identity.to_string());
        }
    }

    fn repository_unavailable(&self, identity: &str, reason: RepoWatchUnavailable) {
        log::warn!("[repowatch] {identity} unavailable: {}", reason.as_str());
        codeterm_events::emit_everywhere(
            &self.app,
            "repository-unavailable",
            serde_json::json!({ "identity": identity, "reason": reason.as_str() }),
        );
    }
}
