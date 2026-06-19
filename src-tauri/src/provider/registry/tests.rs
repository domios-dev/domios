use std::sync::Arc;

use super::ProviderRegistry;
use crate::agent::chat_types::StructuredChat;
use crate::agent::provider::{ModelInfo, ProviderTransport};
use crate::agent::registry::AgentRegistry;
use crate::chatbackend::{ChatBackendError, ChatBackendProvider, ChatBackendRegistry, PresetInfo};

struct Shell {
    id: &'static str,
    spawnable: bool,
}

impl ChatBackendProvider for Shell {
    fn id(&self) -> &str {
        self.id
    }
    fn spawnable(&self) -> bool {
        self.spawnable
    }
    fn open_session(&self, _: serde_json::Value) -> Result<String, ChatBackendError> {
        Ok("s".into())
    }
    fn send_message(&self, _: &str, _: &str) -> Result<(), ChatBackendError> {
        Ok(())
    }
    fn poll(&self, _: &str, _: Option<&str>) -> Result<StructuredChat, ChatBackendError> {
        Ok(StructuredChat::default())
    }
    fn close_session(&self, _: &str) -> Result<(), ChatBackendError> {
        Ok(())
    }
    fn list_models(&self) -> Result<Vec<ModelInfo>, ChatBackendError> {
        Ok(vec![ModelInfo {
            id: "qwen".into(),
            display_name: "Qwen".into(),
            description: None,
        }])
    }
    fn list_presets(&self) -> Result<Vec<PresetInfo>, ChatBackendError> {
        Ok(vec![PresetInfo {
            id: "p1".into(),
            name: "Preset 1".into(),
            description: None,
        }])
    }
}

fn registry() -> ProviderRegistry {
    let agents = Arc::new(AgentRegistry::new());
    let backends = Arc::new(ChatBackendRegistry::new());
    backends.register(
        "shell",
        Arc::new(Shell {
            id: "shell",
            spawnable: true,
        }),
    );
    backends.register(
        "npc",
        Arc::new(Shell {
            id: "npc",
            spawnable: false,
        }),
    );
    ProviderRegistry::new(agents, backends)
}

#[test]
fn describe_all_tags_chat_backend_transport_and_filters_spawnable() {
    let reg = registry();
    let descs = reg.describe_all();

    let shell = descs
        .iter()
        .find(|d| d.id == "shell")
        .expect("spawnable listed");
    assert_eq!(shell.transport, ProviderTransport::ChatBackend);
    assert_eq!(shell.presets.len(), 1);
    assert!(shell.supports_chat_ui);
    assert!(
        !descs.iter().any(|d| d.id == "npc"),
        "non-spawnable backend hidden"
    );
}

#[test]
fn transport_of_routes_by_store() {
    let reg = registry();
    assert_eq!(
        reg.transport_of("shell"),
        Some(ProviderTransport::ChatBackend)
    );
    assert_eq!(
        reg.transport_of("npc"),
        Some(ProviderTransport::ChatBackend)
    );
    assert_eq!(reg.transport_of("unknown"), None);
}

#[test]
fn spawnable_ids_excludes_backend_only() {
    let reg = registry();
    let ids = reg.spawnable_ids();
    assert!(ids.contains(&"shell".to_string()));
    assert!(!ids.contains(&"npc".to_string()));
}

#[test]
fn readiness_gate_unknown_provider() {
    let reg = registry();
    // An id not registered as an agent must not report Ready.
    assert!(!matches!(
        reg.wait_for_agent_startup("nope"),
        crate::agent::registry::ProviderStartupStatus::Ready
    ));
}
