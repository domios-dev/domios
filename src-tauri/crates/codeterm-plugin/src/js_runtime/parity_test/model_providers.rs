use super::super::{model_providers::JsModelProviderConfig, PluginVm};
use crate::model_providers::{ConfigUpdate, ModelProviderConfig, ModelProviderDefinition};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc};

struct Native;
impl ModelProviderConfig for Native {
    fn path(&self) -> Result<PathBuf, String> {
        Ok("fixture/config.json".into())
    }
    fn key_env_name(&self, id: &str) -> Result<String, String> {
        Ok(format!("FIXTURE_{}", id.to_uppercase()))
    }
    fn read(&self, config: &Value) -> Result<Vec<ModelProviderDefinition>, String> {
        serde_json::from_value(config["providers"].clone()).map_err(|_| "invalid".into())
    }
    fn apply(
        &self,
        config: &Value,
        providers: &[ModelProviderDefinition],
    ) -> Result<ConfigUpdate, String> {
        let mut result = config.clone();
        result["providers"] = json!(providers);
        Ok(ConfigUpdate {
            changed: &result != config,
            config: result,
            conflicts: vec![],
        })
    }
}

#[test]
fn parity_model_provider_config_native_and_js_preserve_the_same_contract() {
    let vm = Arc::new(PluginVm::from_source("fixture", r#"module.exports.default = {
        modelProviderConfigPath() { return "fixture/config.json"; },
        modelProviderKeyEnvName(id) { return "FIXTURE_" + id.toUpperCase(); },
        readModelProviders(config) { return config.providers; },
        applyModelProviders(config, providers) {
            const next = Object.assign({}, config, {providers});
            return {config: next, changed: JSON.stringify(next) !== JSON.stringify(config), conflicts: []};
        }
    };"#.into()).unwrap());
    let js = JsModelProviderConfig::new(vm);
    let native = Native;
    let providers: Vec<ModelProviderDefinition> = serde_json::from_value(json!([{
        "id":"endpoint", "kind":"openai-compatible", "baseUrl":"http://localhost:1234/v1", "models":["a"],
        "secretName":"slot", "keyEnv":"FIXTURE_ENDPOINT"
    }])).unwrap();
    let config = json!({"foreign":{"untouched":true}, "providers":providers});
    assert_eq!(native.path().unwrap(), js.path().unwrap());
    assert_eq!(
        native.key_env_name("endpoint").unwrap(),
        js.key_env_name("endpoint").unwrap()
    );
    assert_eq!(native.read(&config).unwrap(), js.read(&config).unwrap());
    for desired in [&providers[..], &[][..]] {
        assert_eq!(
            native.apply(&config, desired).unwrap(),
            js.apply(&config, desired).unwrap()
        );
    }
}

#[test]
fn parity_model_provider_config_declaration_is_opt_in() {
    use crate::provider::AgentProvider;
    let js = super::load_inline_js("no-capability", "module.exports.default = {};");
    assert!(js.model_provider_config().is_none());
    let plugin = super::load_js("opencode");
    assert!(plugin.model_provider_config().is_some());
    let native = crate::agent_claude::provider::ClaudeProvider::new();
    assert!(native.model_provider_config().is_none());
}

#[test]
fn parity_model_provider_config_cannot_escape_the_plugin_filesystem_grant() {
    let dir = tempfile::tempdir().unwrap();
    let sandbox = super::super::host_api::PluginSandbox::restricted(
        vec![dir.path().to_owned()],
        vec![dir.path().to_owned()],
    );
    let vm = Arc::new(
        PluginVm::from_source_sandboxed(
            "confined",
            r#"module.exports.default = {
        modelProviderConfigPath() { return "/outside-grant/config.json"; }
    };"#
            .into(),
            None,
            Arc::new(sandbox),
        )
        .unwrap(),
    );
    assert!(JsModelProviderConfig::new(vm).path().is_err());
}
