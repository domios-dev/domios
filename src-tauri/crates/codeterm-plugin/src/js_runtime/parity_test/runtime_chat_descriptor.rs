use super::*;

struct RuntimeChatNativeStub(Option<crate::agent_runtime::AgentRuntimeCapability>, bool);

impl ChatBackendProvider for RuntimeChatNativeStub {
    fn general_agent_offer_when_absent(&self) -> bool {
        self.1
    }

    fn agent_runtime(&self) -> Option<crate::agent_runtime::AgentRuntimeCapability> {
        self.0.clone()
    }

    fn id(&self) -> &str {
        "session-config-fixture"
    }

    fn supports_session_config(&self) -> bool {
        true
    }

    fn open_session(&self, _: serde_json::Value) -> Result<String, ChatBackendError> {
        unreachable!("descriptor parity does not open a session")
    }

    fn send_message(&self, _: &str, _: &str) -> Result<(), ChatBackendError> {
        unreachable!("descriptor parity does not send messages")
    }

    fn poll(
        &self,
        _: &str,
        _: Option<&str>,
    ) -> Result<codeterm_types::agent::chat_types::StructuredChat, ChatBackendError> {
        unreachable!("descriptor parity does not poll sessions")
    }

    fn close_session(&self, _: &str) -> Result<(), ChatBackendError> {
        unreachable!("descriptor parity does not close sessions")
    }

    fn list_models(&self) -> Result<Vec<ModelInfo>, ChatBackendError> {
        unreachable!("descriptor parity does not list models")
    }
}

/// Chosen by capability, so the parity check never names a provider.
fn bundled_manifest_declaring_agent_runtime() -> String {
    let plugins = crate::test_support::fixture_manifest_dir().join("../../../plugins");
    let mut manifests: Vec<_> = std::fs::read_dir(&plugins)
        .unwrap()
        .filter_map(|entry| std::fs::read_to_string(entry.ok()?.path().join("plugin.json")).ok())
        .collect();
    manifests.sort();
    manifests
        .into_iter()
        .find(|text| {
            serde_json::from_str::<serde_json::Value>(text)
                .is_ok_and(|value| value["capabilities"]["agentRuntime"].is_object())
        })
        .expect("a bundled plugin declares agentRuntime")
}

#[test]
fn runtime_chat_descriptor_preserves_manifest_capability_native_js_parity() {
    let manifest_text = bundled_manifest_declaring_agent_runtime();
    for (enabled, offer) in [(false, false), (false, true), (true, false), (true, true)] {
        let mut value: serde_json::Value = serde_json::from_str(&manifest_text).unwrap();
        value["generalAgentOfferWhenAbsent"] = offer.into();
        value["id"] = serde_json::json!("runtime-chat-fixture");
        if !enabled {
            value["capabilities"]
                .as_object_mut()
                .unwrap()
                .remove("agentRuntime");
        }
        let manifest: super::super::PluginManifest = serde_json::from_value(value).unwrap();
        let vm = super::super::PluginVm::from_source(
            "runtime-chat-fixture",
            "module.exports.default = {};".into(),
        )
        .unwrap();
        let js = super::super::JsChatBackend::from_manifest(std::sync::Arc::new(vm), &manifest);
        let native = RuntimeChatNativeStub(manifest.capabilities.agent_runtime.clone(), offer);
        assert_eq!(js.agent_runtime(), native.agent_runtime());
        let registry = ChatBackendRegistry::new();
        registry.register("runtime-chat-fixture", std::sync::Arc::new(js));
        let descriptor = registry.descriptor("runtime-chat-fixture").unwrap();
        assert_eq!(
            descriptor.general_agent_offer_when_absent,
            native.describe().general_agent_offer_when_absent
        );
        assert_eq!(descriptor.general_agent_offer_when_absent, offer);
        assert_eq!(descriptor.agent_runtime, native.describe().agent_runtime);
        assert_eq!(descriptor.agent_runtime.is_some(), enabled);
    }
}
