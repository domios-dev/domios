use crate::{
    js_runtime::{IntegrationKind, PluginManifest},
    provider::AgentProvider,
};

use super::{load_inline_js, load_js, require_plugins};

#[test]
fn opencode_integration_kind_native_js_parity() {
    let Some(plugins_root) = require_plugins() else {
        return;
    };
    let manifest = PluginManifest::load(&plugins_root.join("provider-opencode/plugin.json"))
        .expect("load OpenCode manifest");
    let manifest_kind = manifest
        .integration
        .as_ref()
        .map(|integration| integration.kind)
        .unwrap_or(IntegrationKind::None);
    let js_provider = load_js("opencode");
    let native_claude = crate::agent_claude::provider::ClaudeProvider::new();
    let js_claude = load_js("claude");

    assert_eq!(manifest_kind, IntegrationKind::PluginFile);
    assert_eq!(js_provider.integration_kind(), manifest_kind);
    assert!(js_provider.has_installable_integration());
    assert_eq!(
        native_claude.integration_kind(),
        IntegrationKind::ClaudeSettingsJson
    );
    assert_eq!(
        native_claude.integration_kind(),
        js_claude.integration_kind()
    );

    let provider_without_integration = load_inline_js("plain", "{}");
    assert_eq!(
        provider_without_integration.integration_kind(),
        IntegrationKind::None
    );
    assert!(!provider_without_integration.has_installable_integration());
}
