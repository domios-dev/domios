//! Minimal user-spawnable chat backend shared by the spawn-routing tests in the
//! cron and memory-worker modules.

use crate::chatbackend::{ChatBackendError, ChatBackendProvider};

pub struct SpawnableTestBackend {
    pub id: &'static str,
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

pub fn registry_with_spawnable(id: &'static str) -> crate::chatbackend::ChatBackendRegistry {
    let registry = crate::chatbackend::ChatBackendRegistry::new();
    registry.register(id, std::sync::Arc::new(SpawnableTestBackend { id }));
    registry
}

struct RuntimeTestBackend(SpawnableTestBackend);

impl ChatBackendProvider for RuntimeTestBackend {
    fn id(&self) -> &str {
        self.0.id()
    }
    fn spawnable(&self) -> bool {
        true
    }
    fn describe(&self) -> crate::agent::provider::ProviderDescriptor {
        let mut descriptor = self.0.describe();
        descriptor.agent_runtime = Some(codeterm_plugin::agent_runtime::AgentRuntimeCapability {
            usage: false,
            runtime: codeterm_plugin::agent_runtime::AgentRuntimeProtocol::StdioJsonrpc,
            command: codeterm_plugin::agent_runtime::AgentRuntimeCommand {
                binary: "fixture-peer".into(),
                args: Vec::new(),
            },
            inherited_environment_exclusions: Vec::new(),
            login: None,
            request_methods: Default::default(),
            notification_methods: Vec::new(),
        });
        descriptor
    }
    fn open_session(&self, config: serde_json::Value) -> Result<String, ChatBackendError> {
        self.0.open_session(config)
    }
    fn send_message(&self, session: &str, text: &str) -> Result<(), ChatBackendError> {
        self.0.send_message(session, text)
    }
    fn poll(
        &self,
        session: &str,
        cursor: Option<&str>,
    ) -> Result<crate::agent::chat_types::StructuredChat, ChatBackendError> {
        self.0.poll(session, cursor)
    }
    fn close_session(&self, session: &str) -> Result<(), ChatBackendError> {
        self.0.close_session(session)
    }
    fn list_models(&self) -> Result<Vec<crate::agent::provider::ModelInfo>, ChatBackendError> {
        self.0.list_models()
    }
}

pub fn registry_with_runtime(id: &'static str) -> crate::chatbackend::ChatBackendRegistry {
    let registry = crate::chatbackend::ChatBackendRegistry::new();
    registry.register(
        id,
        std::sync::Arc::new(RuntimeTestBackend(SpawnableTestBackend { id })),
    );
    registry
}
