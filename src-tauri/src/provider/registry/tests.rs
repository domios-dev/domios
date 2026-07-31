use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;

use super::ProviderRegistry;
use crate::agent::chat_types::StructuredChat;
use crate::agent::provider::{ModelInfo, ProviderTransport};
use crate::agent::registry::AgentRegistry;
use crate::chatbackend::{ChatBackendError, ChatBackendProvider, ChatBackendRegistry, PresetInfo};

struct Shell {
    id: &'static str,
    spawnable: bool,
    model_list_calls: Arc<AtomicUsize>,
    models: Arc<RwLock<Vec<ModelInfo>>>,
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
        self.model_list_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.models.read().clone())
    }
    fn list_presets(&self) -> Result<Vec<PresetInfo>, ChatBackendError> {
        Ok(vec![PresetInfo {
            id: "p1".into(),
            name: "Preset 1".into(),
            description: None,
        }])
    }
}

fn registry() -> (
    ProviderRegistry,
    Arc<AtomicUsize>,
    Arc<RwLock<Vec<ModelInfo>>>,
) {
    let agents = Arc::new(AgentRegistry::new());
    let backends = Arc::new(ChatBackendRegistry::new());
    let model_list_calls = Arc::new(AtomicUsize::new(0));
    let models = Arc::new(RwLock::new(vec![ModelInfo {
        id: "qwen".into(),
        display_name: "Qwen".into(),
        description: None,
        available: None,
    }]));
    backends.register(
        "shell",
        Arc::new(Shell {
            id: "shell",
            spawnable: true,
            model_list_calls: model_list_calls.clone(),
            models: models.clone(),
        }),
    );
    backends.register(
        "npc",
        Arc::new(Shell {
            id: "npc",
            spawnable: false,
            model_list_calls: Arc::new(AtomicUsize::new(0)),
            models: Arc::new(RwLock::new(Vec::new())),
        }),
    );
    (
        ProviderRegistry::new(agents, backends),
        model_list_calls,
        models,
    )
}

#[test]
fn describe_all_tags_chat_backend_transport_and_filters_spawnable() {
    let (reg, model_list_calls, _) = registry();
    let descs = reg.describe_all();

    let shell = descs
        .iter()
        .find(|d| d.id == "shell")
        .expect("spawnable listed");
    assert_eq!(shell.transport, ProviderTransport::ChatBackend);
    assert_eq!(model_list_calls.load(Ordering::SeqCst), 1);
    assert_eq!(shell.available_models[0].id, "qwen");
    assert_eq!(shell.presets[0].id, "p1");
    assert!(shell.supports_chat_ui);
    assert!(
        !descs.iter().any(|d| d.id == "npc"),
        "non-spawnable backend hidden"
    );
}

#[test]
fn describe_all_reflects_plugin_catalog_changes_without_reregistering() {
    let (reg, model_list_calls, models) = registry();
    assert_eq!(reg.describe_all()[0].available_models[0].id, "qwen");

    *models.write() = vec![ModelInfo {
        id: "llama".into(),
        display_name: "Llama".into(),
        description: None,
        available: None,
    }];

    assert_eq!(reg.describe_all()[0].available_models[0].id, "llama");
    assert_eq!(model_list_calls.load(Ordering::SeqCst), 2);
}

#[test]
fn transport_of_routes_by_store() {
    let (reg, _, _) = registry();
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
    let (reg, _, _) = registry();
    let ids = reg.spawnable_ids();
    assert!(ids.contains(&"shell".to_string()));
    assert!(!ids.contains(&"npc".to_string()));
}

#[test]
fn readiness_gate_unknown_provider() {
    let (reg, _, _) = registry();
    // An id not registered as an agent must not report Ready.
    assert!(!matches!(
        reg.wait_for_agent_startup("nope"),
        crate::agent::registry::ProviderStartupStatus::Ready
    ));
}
