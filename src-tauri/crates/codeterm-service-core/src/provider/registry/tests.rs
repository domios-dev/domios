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

#[test]
fn bundled_cloud_runtime_is_one_unavailable_execution_mode() {
    use crate::agent::provider::AgentProvider;
    let agents = Arc::new(AgentRegistry::new());
    let plugins_root = crate::test_support::fixture_manifest_dir().join("../../../plugins");
    let provider = std::fs::read_dir(plugins_root)
        .expect("bundled plugins directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("plugin.json"))
        .filter(|path| path.is_file())
        .filter_map(|path| codeterm_plugin::js_runtime::load_one(&path).ok())
        .find(|provider| {
            provider.agent_runtime().is_some()
                && provider
                    .remote_task_capability()
                    .is_some_and(|capability| capability.mode == "cloud")
        })
        .expect("a bundled provider declares a cloud runtime capability");
    let runtime = provider
        .agent_runtime()
        .expect("Bundled runtime capability");
    let provider_id = provider.id().to_owned();
    agents.register_provider(Box::new(provider));
    let registry = ProviderRegistry::new(agents, Arc::new(ChatBackendRegistry::new()));
    let descriptors = registry.describe_all();
    assert_eq!(descriptors.len(), 1);
    assert_eq!(descriptors[0].id, provider_id);
    assert_eq!(descriptors[0].agent_runtime.as_ref(), Some(&runtime));
    assert_eq!(descriptors[0].transport, ProviderTransport::ChatBackend);
    assert_eq!(
        registry.transport_of(&provider_id),
        Some(ProviderTransport::ChatBackend)
    );
    let remote_task = serde_json::to_value(descriptors[0].remote_task.as_ref()).unwrap();
    assert_eq!(remote_task["mode"], "cloud");
    assert_eq!(remote_task["availability"]["status"], "unavailable");
    assert_eq!(remote_task["environments"], serde_json::json!([]));
    assert!(remote_task["availability"]["reason"]
        .as_str()
        .is_some_and(|reason| !reason.is_empty()));
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
        default_reasoning_effort: None,
        id: "qwen".into(),
        display_name: "Qwen".into(),
        description: None,
        available: None,
        availability: None,
        primary: None,
        featured: None,
        badge: None,
        group: None,
        reasoning_efforts: None,
    }]));
    backends.register_and_wait(
        "shell",
        Arc::new(Shell {
            id: "shell",
            spawnable: true,
            model_list_calls: model_list_calls.clone(),
            models: models.clone(),
        }),
    );
    backends.register_and_wait(
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
fn chatbackend_list_models_populates_descriptor_catalog() {
    let (reg, model_list_calls, _) = registry();
    let descs = reg.describe_all();

    let shell = descs
        .iter()
        .find(|d| d.id == "shell")
        .expect("spawnable listed");
    assert_eq!(shell.transport, ProviderTransport::ChatBackend);
    assert_eq!(
        model_list_calls.load(Ordering::SeqCst),
        1,
        "registration resolves list_models exactly once"
    );
    assert_eq!(shell.available_models[0].id, "qwen");
    assert_eq!(shell.default_model, None);
    assert_eq!(shell.presets[0].id, "p1");
    assert!(shell.supports_chat_ui);
    assert!(
        !descs.iter().any(|d| d.id == "npc"),
        "non-spawnable backend hidden"
    );
}

#[test]
fn describe_all_reads_registration_snapshot_until_provider_refresh() {
    let (reg, model_list_calls, models) = registry();
    assert_eq!(reg.describe_all()[0].available_models[0].id, "qwen");

    *models.write() = vec![ModelInfo {
        default_reasoning_effort: None,
        id: "llama".into(),
        display_name: "Llama".into(),
        description: None,
        available: None,
        availability: None,
        primary: None,
        featured: None,
        badge: None,
        group: None,
        reasoning_efforts: None,
    }];

    assert_eq!(
        reg.describe_all()[0].available_models[0].id,
        "qwen",
        "descriptor reads must not invoke the live capability"
    );
    assert_eq!(model_list_calls.load(Ordering::SeqCst), 1);

    reg.chat_backends.register_and_wait(
        "shell",
        Arc::new(Shell {
            id: "shell",
            spawnable: true,
            model_list_calls: model_list_calls.clone(),
            models,
        }),
    );

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

#[test]
fn chat_backend_cli_status_requires_no_binary_or_installer() {
    let (registry, _, _) = registry();
    let status = registry.cli_status("shell", "windows").unwrap();
    assert!(!status.required);
    assert_eq!(status.binary, None);
    assert_eq!(status.installed, None);
    assert_eq!(status.installer, None);
    assert_eq!(status.updater, None);
    assert!(registry.cli_status("missing", "linux").is_err());
}

#[test]
fn runtime_capability_selects_chat_transport_and_retains_cli_controls() {
    use crate::agent::provider::AgentProvider;
    let manifest = codeterm_plugin::js_runtime::PluginManifest::from_json_str(&serde_json::json!({
        "id":"runtime-fixture", "version":"1.0.0", "displayName":"Runtime fixture", "cliBinary":"fixture-cli",
        "capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},
            "agentRuntime":{"runtime":"stdio-jsonrpc","command":{"binary":"fixture-cli","args":["serve"]},
                "requestMethods":{"initialize":["handshake"]},"notificationMethods":[]}},
        "permissions":{"subprocess":{"allow":["fixture-cli"]}}, "installer":{"macos":"install fixture-cli"}
    }).to_string()).unwrap();
    let vm = Arc::new(
        codeterm_plugin::js_runtime::PluginVm::from_source(
            "runtime-fixture",
            "module.exports={}".into(),
        )
        .unwrap(),
    );
    let provider = codeterm_plugin::js_runtime::JsProvider::new(manifest, vm);
    assert_eq!(
        provider.describe().transport,
        ProviderTransport::ChatBackend
    );
    assert!(provider.describe().agent_runtime.is_some());
    let agents = Arc::new(AgentRegistry::new());
    agents.register_provider(Box::new(provider));
    let registry = ProviderRegistry::new(agents, Arc::new(ChatBackendRegistry::new()));
    assert_eq!(
        registry.transport_of("runtime-fixture"),
        Some(ProviderTransport::ChatBackend)
    );
    assert!(registry.is_spawnable_chat_backend("runtime-fixture"));
    assert_eq!(
        registry.describe_all()[0].cli_binary.as_deref(),
        Some("fixture-cli")
    );
    assert!(
        registry
            .cli_status("runtime-fixture", "macos")
            .unwrap()
            .required
    );
}

#[test]
fn remote_mode_keeps_one_provider_and_its_default_local_transport() {
    use crate::agent::provider::AgentProvider;
    for runtime in [false, true] {
        let id = if runtime {
            "opaque-local-runtime"
        } else {
            "opaque-local-pty"
        };
        let mut value = serde_json::json!({"id":id,"version":"0.1.0","displayName":"Fixture",
        "capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true},"remoteTask":{
            "mode":"opaque-cloud","displayName":"Fixture Cloud","backend":"fixture","availability":{"status":"unavailable","reason":"fixture"},
            "environments":[],"followUp":false,"cancel":false,"applyDiff":false
        }}});
        if runtime {
            value["capabilities"]["agentRuntime"] = serde_json::json!({"runtime":"stdio-jsonrpc","command":{"binary":"opaque-cli","args":[]},"requestMethods":{"initialize":["initialize"]},"notificationMethods":[]});
        }
        let manifest = serde_json::from_value(value).unwrap();
        let vm = Arc::new(
            codeterm_plugin::js_runtime::PluginVm::from_source(
                id,
                "module.exports.default={};".into(),
            )
            .unwrap(),
        );
        let backend =
            codeterm_plugin::js_runtime::JsChatBackend::from_manifest(vm.clone(), &manifest);
        let provider = codeterm_plugin::js_runtime::JsProvider::new(manifest, vm);
        assert!(provider.remote_task_capability().is_some());
        let agents = Arc::new(AgentRegistry::new());
        agents.register_provider(Box::new(provider));
        let backends = Arc::new(ChatBackendRegistry::new());
        backends.register(id, Arc::new(backend));
        let registry = ProviderRegistry::new(agents, backends);
        let descriptors = registry.describe_all();
        assert_eq!(descriptors.len(), 1);
        assert_eq!(descriptors[0].id, id);
        assert_eq!(descriptors[0].agent_runtime.is_some(), runtime);
        assert!(descriptors[0].remote_task.is_some());
        assert_eq!(
            registry.transport_of(id),
            Some(if runtime {
                ProviderTransport::ChatBackend
            } else {
                ProviderTransport::Pty
            })
        );
    }
}
