use super::*;

pub(super) fn load_sqlite_fixture_js(id: &str) -> JsProvider {
    let directory = workspace_plugins_root().join(format!("provider-{id}"));
    let mut json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(directory.join("plugin.json")).unwrap())
            .unwrap();
    json["cliCompatibility"]["supported"] = serde_json::json!(["=1.0.0"]);
    let json = json.to_string();
    let manifest = super::super::PluginManifest::from_json_str(&json).unwrap();
    let source = std::fs::read_to_string(directory.join("plugin.js")).unwrap();
    let vm = super::super::provider_observation::with_fixture_version(Some("1.0.0"), || {
        super::super::PluginVm::from_source_with(id, source, Some(json)).unwrap()
    });
    JsProvider::new(manifest, std::sync::Arc::new(vm))
}
