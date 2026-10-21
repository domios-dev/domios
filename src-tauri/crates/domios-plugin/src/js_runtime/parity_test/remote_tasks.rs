use super::*;
use crate::remote_tasks::{RemoteTaskCapability, RemoteTaskDiscovery};

struct RemoteNativeStub {
    capability: RemoteTaskCapability,
    discovery: Option<RemoteTaskDiscovery>,
}

impl ChatBackendProvider for RemoteNativeStub {
    fn id(&self) -> &str {
        "remote-fixture"
    }

    fn remote_task_capability(&self) -> Option<RemoteTaskCapability> {
        Some(self.capability.clone())
    }

    fn discover_remote_task(&self) -> Result<Option<RemoteTaskDiscovery>, ChatBackendError> {
        Ok(self.discovery.clone())
    }

    fn open_session(&self, _: serde_json::Value) -> Result<String, ChatBackendError> {
        unreachable!()
    }

    fn send_message(&self, _: &str, _: &str) -> Result<(), ChatBackendError> {
        unreachable!()
    }

    fn poll(
        &self,
        _: &str,
        _: Option<&str>,
    ) -> Result<codeterm_types::agent::chat_types::StructuredChat, ChatBackendError> {
        unreachable!()
    }

    fn close_session(&self, _: &str) -> Result<(), ChatBackendError> {
        unreachable!()
    }

    fn list_models(&self) -> Result<Vec<ModelInfo>, ChatBackendError> {
        Ok(Vec::new())
    }
}

#[test]
fn remote_task_capability_native_js_parity_and_manifest_round_trip() {
    let capability = serde_json::json!({
        "backend":"fixture", "mode":"cloud", "displayName":"Cloud",
        "availability":{"status":"unavailable","reason":"Connect an account"},
        "environments":[],"followUp":false,"cancel":false,"applyDiff":false
    });
    let manifest_json = serde_json::json!({
        "id":"remote-fixture","version":"0.1.0","displayName":"Fixture",
        "capabilities":{"agentProvider":true,"remoteTask":capability,"chatBackend":{"spawnable":true}}
    })
    .to_string();
    let manifest: super::super::PluginManifest = serde_json::from_str(&manifest_json).unwrap();
    let round_trip = serde_json::to_value(&manifest.capabilities).unwrap();
    assert_eq!(round_trip["remoteTask"], capability);
    let vm = super::super::PluginVm::from_source_with(
        "remote-fixture",
        "module.exports.default = {};".into(),
        Some(manifest_json.clone()),
    )
    .unwrap();
    let js = super::super::JsChatBackend::from_manifest(std::sync::Arc::new(vm), &manifest);
    let native = RemoteNativeStub {
        capability: serde_json::from_value(capability.clone()).unwrap(),
        discovery: None,
    };
    let agent = super::super::JsProvider::new(
        manifest.clone(),
        std::sync::Arc::new(
            super::super::PluginVm::from_source_with(
                "remote-fixture",
                "module.exports.default = {};".into(),
                Some(manifest_json.clone()),
            )
            .unwrap(),
        ),
    );
    assert_eq!(agent.describe().remote_task, native.describe().remote_task);
    assert_eq!(agent.describe().id, js.describe().id);
    let native_agent = RemoteAgentNativeStub(serde_json::from_value(capability).unwrap());
    assert_eq!(
        agent.describe().remote_task,
        native_agent.describe().remote_task
    );
    assert_eq!(js.describe().remote_task, native.describe().remote_task);
    assert_eq!(
        js.describe().transport,
        crate::provider::ProviderTransport::ChatBackend
    );
}

#[test]
fn remote_task_runtime_discovery_native_js_parity() {
    let capability = serde_json::json!({
        "backend":"fixture", "mode":"cloud", "displayName":"Cloud",
        "availability":{"status":"unavailable","reason":"Sign in"},
        "environments":[],"followUp":false,"cancel":false,"applyDiff":false
    });
    let discovery = serde_json::json!({
        "availability":{"status":"available"},
        "environments":[{"id":"env-fixture","label":"Fixture environment","accountRef":"fixture-auth"}]
    });
    let manifest_json = serde_json::json!({
        "id":"remote-runtime-fixture","version":"0.1.0","displayName":"Fixture",
        "capabilities":{"agentProvider":true,"remoteTask":capability,"chatBackend":{"spawnable":true}}
    })
    .to_string();
    let manifest: super::super::PluginManifest = serde_json::from_str(&manifest_json).unwrap();
    let discovery_json = discovery.to_string();
    let vm = super::super::PluginVm::from_source_with(
        "remote-runtime-fixture",
        format!("module.exports.default = {{discoverRemoteTask: function() {{ return {discovery_json}; }}}};"),
        Some(manifest_json.clone()),
    )
    .unwrap();
    let js = super::super::JsChatBackend::from_manifest(std::sync::Arc::new(vm), &manifest);
    let native = RemoteNativeStub {
        capability: serde_json::from_value(
            serde_json::from_str::<serde_json::Value>(&manifest_json).unwrap()["capabilities"]
                ["remoteTask"]
                .clone(),
        )
        .unwrap(),
        discovery: Some(serde_json::from_value(discovery.clone()).unwrap()),
    };

    let js_registry = ChatBackendRegistry::new();
    js_registry.register_and_wait("remote-runtime-fixture", std::sync::Arc::new(js));
    let native_registry = ChatBackendRegistry::new();
    native_registry.register_and_wait("remote-runtime-fixture", std::sync::Arc::new(native));
    let js_capability = js_registry
        .descriptor("remote-runtime-fixture")
        .unwrap()
        .remote_task
        .unwrap();
    let native_capability = native_registry
        .descriptor("remote-runtime-fixture")
        .unwrap()
        .remote_task
        .unwrap();
    assert_eq!(js_capability, native_capability);
    let runtime_capability = serde_json::to_value(js_capability).unwrap();
    assert_eq!(
        runtime_capability["availability"],
        serde_json::json!({"status":"available"})
    );
    assert_eq!(
        runtime_capability["environments"],
        discovery["environments"]
    );
}

struct RemoteAgentNativeStub(RemoteTaskCapability);

#[async_trait::async_trait]
impl AgentProvider for RemoteAgentNativeStub {
    fn id(&self) -> &str {
        "remote-fixture"
    }
    fn display_name(&self) -> &str {
        "Fixture"
    }
    fn remote_task_capability(&self) -> Option<RemoteTaskCapability> {
        Some(self.0.clone())
    }
    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> crate::provider::ReadinessStrategy {
        crate::provider::ReadinessStrategy::PollShellPrompt {
            timeout: std::time::Duration::from_secs(1),
        }
    }
    fn task_delivery(&self) -> crate::provider::TaskDelivery {
        crate::provider::TaskDelivery::PtyInput
    }
    fn detect_from_title(&self, _: &str) -> bool {
        false
    }
    fn detect_from_output(&self, _: &str) -> bool {
        false
    }
    fn detect_session_id(
        &self,
        _: &str,
        _: &[String],
        _: Option<std::time::Duration>,
    ) -> Option<String> {
        None
    }
    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        None
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
}

#[test]
fn remote_mode_adapter_never_replaces_or_unregisters_the_local_model_catalogue() {
    let capability = serde_json::from_value(serde_json::json!({
        "backend":"fixture","mode":"cloud","displayName":"Cloud","availability":{"status":"available"},
        "environments":[],"followUp":false,"cancel":false,"applyDiff":false
    })).unwrap();
    let defaults = crate::model_catalog::RegisteredDefaults {
        models: Vec::new(),
        default: Some("opaque-local-model".into()),
    };
    crate::model_catalog::register("remote-fixture", defaults);
    let registry = ChatBackendRegistry::new();
    registry.register_and_wait(
        "remote-fixture",
        std::sync::Arc::new(RemoteNativeStub {
            capability,
            discovery: None,
        }),
    );
    assert_eq!(
        crate::model_catalog::registered_snapshot("remote-fixture")
            .default
            .as_deref(),
        Some("opaque-local-model")
    );
    registry.unregister("remote-fixture");
    assert_eq!(
        crate::model_catalog::registered_snapshot("remote-fixture")
            .default
            .as_deref(),
        Some("opaque-local-model")
    );
    crate::model_catalog::unregister("remote-fixture");
}
