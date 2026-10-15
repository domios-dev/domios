use super::provider::AgentProvider;
pub use codeterm_agent_model::agent::registry::{
    ProviderStartupStatus, SessionMatchMode, SessionSearchError, SessionSearchHit,
    SessionSearchOpts, SessionSearchResult, SessionSearchWarning, SessionSkipReason,
    SessionSnippet, DEFAULT_SESSION_FILE_BYTE_CAP, PROVIDER_STARTUP_WAIT,
};
use std::sync::Arc;
type ModelRegistry = codeterm_agent_model::agent::registry::AgentRegistry;

#[derive(Default)]
pub struct AgentRegistry(ModelRegistry);

impl std::ops::Deref for AgentRegistry {
    type Target = ModelRegistry;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn register_provider(&self, provider: Box<dyn AgentProvider>) {
        self.0.register_provider(provider);
    }
    pub fn unregister_provider(&self, id: &str) -> bool {
        self.0.unregister_provider(id)
    }
    pub fn native_fallback(&self, id: &str) -> Option<Arc<dyn AgentProvider>> {
        self.0.native_fallback(id)
    }
}
