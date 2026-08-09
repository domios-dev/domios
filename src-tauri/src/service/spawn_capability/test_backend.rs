//! Minimal user-spawnable chat backend shared by the spawn-routing tests in the
//! cron and memory-worker modules.

use crate::chatbackend::{ChatBackendError, ChatBackendProvider};

pub(crate) struct SpawnableTestBackend {
    pub(crate) id: &'static str,
}

impl ChatBackendProvider for SpawnableTestBackend {
    fn id(&self) -> &str {
        self.id
    }

    fn spawnable(&self) -> bool {
        true
    }

    fn open_session(&self, _: serde_json::Value) -> Result<String, ChatBackendError> {
        Ok("session".into())
    }

    fn send_message(&self, _: &str, _: &str) -> Result<(), ChatBackendError> {
        Ok(())
    }

    fn poll(
        &self,
        _: &str,
        _: Option<&str>,
    ) -> Result<crate::agent::chat_types::StructuredChat, ChatBackendError> {
        Ok(crate::agent::chat_types::StructuredChat::default())
    }

    fn close_session(&self, _: &str) -> Result<(), ChatBackendError> {
        Ok(())
    }

    fn list_models(&self) -> Result<Vec<crate::agent::provider::ModelInfo>, ChatBackendError> {
        Ok(Vec::new())
    }
}

pub(crate) fn registry_with_spawnable(id: &'static str) -> crate::chatbackend::ChatBackendRegistry {
    let registry = crate::chatbackend::ChatBackendRegistry::new();
    registry.register(id, std::sync::Arc::new(SpawnableTestBackend { id }));
    registry
}
