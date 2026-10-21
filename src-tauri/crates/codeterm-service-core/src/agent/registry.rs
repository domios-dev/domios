use super::provider::AgentProvider;
pub use codeterm_agent_model::agent::registry::{
    ProviderStartupStatus, SessionMatchMode, SessionSearchError, SessionSearchHit,
    SessionSearchOpts, SessionSearchResult, SessionSearchWarning, SessionSkipReason,
    SessionSnippet, DEFAULT_SESSION_FILE_BYTE_CAP, PROVIDER_STARTUP_WAIT,
};
use std::sync::Arc;
type ModelRegistry = codeterm_agent_model::agent::registry::AgentRegistry;
type ProviderChanged = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Default)]
pub struct AgentRegistry {
    model: ModelRegistry,
    provider_changed: parking_lot::Mutex<Option<ProviderChanged>>,
}

impl std::ops::Deref for AgentRegistry {
    type Target = ModelRegistry;
    fn deref(&self) -> &Self::Target {
        &self.model
    }
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn register_provider(&self, provider: Box<dyn AgentProvider>) {
        let id = provider.id().to_owned();
        self.model.register_provider(provider);
        self.publish_provider_changed(&id);
    }
    pub fn unregister_provider(&self, id: &str) -> bool {
        let removed = self.model.unregister_provider(id);
        if removed {
            self.publish_provider_changed(id);
        }
        removed
    }
    pub fn native_fallback(&self, id: &str) -> Option<Arc<dyn AgentProvider>> {
        self.model.native_fallback(id)
    }

    pub(crate) fn observe_provider_changes(&self, callback: ProviderChanged) {
        *self.provider_changed.lock() = Some(callback);
    }

    fn publish_provider_changed(&self, id: &str) {
        let callback = self.provider_changed.lock().clone();
        if let Some(callback) = callback {
            callback(id);
        }
    }
}

#[cfg(test)]
mod tests;
