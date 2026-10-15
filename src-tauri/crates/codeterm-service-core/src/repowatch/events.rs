use codeterm_repowatch::{RepoEventSink, RepoWatchUnavailable};

pub struct TauriRepoEventSink {
    ui: std::sync::Arc<dyn crate::ports::host::UiEvents>,
    service: parking_lot::RwLock<std::sync::Weak<crate::service::ServiceContext>>,
}

impl TauriRepoEventSink {
    pub fn bind_service(&self, service: std::sync::Weak<crate::service::ServiceContext>) {
        *self.service.write() = service;
    }
    pub fn new(ui: std::sync::Arc<dyn crate::ports::host::UiEvents>) -> Self {
        Self {
            ui,
            service: Default::default(),
        }
    }
}

impl RepoEventSink for TauriRepoEventSink {
    fn repository_changed(&self, identity: &str) {
        self.ui.emit(
            "repository-changed",
            serde_json::json!({ "identity": identity }),
        );
    }

    fn repository_head_changed(&self, identity: &str) {
        let service = self.service.read().upgrade();
        if let Some(service) = service {
            service.offer_merged_lane_cleanup(identity.to_string());
        }
    }

    fn repository_unavailable(&self, identity: &str, reason: RepoWatchUnavailable) {
        log::warn!("[repowatch] {identity} unavailable: {}", reason.as_str());
        self.ui.emit(
            "repository-unavailable",
            serde_json::json!({ "identity": identity, "reason": reason.as_str() }),
        );
    }
}
