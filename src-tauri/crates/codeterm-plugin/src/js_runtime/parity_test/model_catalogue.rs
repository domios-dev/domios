use super::super::{PluginManifest, PluginVm};
use super::*;

struct FreeFormNativeStub;

#[async_trait::async_trait]
impl AgentProvider for FreeFormNativeStub {
    fn id(&self) -> &str {
        "opencode"
    }

    fn display_name(&self) -> &str {
        "OpenCode"
    }

    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }
}
#[test]
fn model_catalogue_authority_native_js_parity() {
    use std::sync::Arc;

    let native_claude = crate::agent_claude::provider::ClaudeProvider::new();
    let js_claude = load_js("claude");
    assert_eq!(
        native_claude.model_catalogue_capability(),
        js_claude.model_catalogue_capability()
    );
    let native_free_form = FreeFormNativeStub;
    let js_free_form = load_js("opencode");
    assert_eq!(
        native_free_form.model_catalogue_capability(),
        js_free_form.model_catalogue_capability()
    );

    let manifest_json = r#"{
        "id":"discover-fixture",
        "version":"0.1.0",
        "displayName":"Discover Fixture",
        "modelCatalogue":{"authority":"authoritative","liveDiscoveryRequired":true},
        "spawn":{"readinessStrategy":{"kind":"fixed_delay","durationMs":0},
                 "taskDelivery":"pty_input","verifySubmit":{"kind":"none"}}
    }"#;
    let manifest: PluginManifest = serde_json::from_str(manifest_json).unwrap();
    let vm = PluginVm::from_source_with(
        "discover-fixture",
        "module.exports.default = {};".into(),
        Some(manifest_json.into()),
    )
    .unwrap();
    let js_discover = JsProvider::new(manifest, Arc::new(vm));
    let native_discover = DiscoverModelsNativeStub;
    assert_eq!(
        native_discover.model_catalogue_capability(),
        js_discover.model_catalogue_capability()
    );
    assert_eq!(
        native_discover.describe().model_catalogue,
        js_discover.describe().model_catalogue
    );
}
