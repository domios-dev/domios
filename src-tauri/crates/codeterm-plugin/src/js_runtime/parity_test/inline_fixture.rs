use super::*;

pub(super) fn provider_fixture(id: &str, relative: &str) -> String {
    let path = workspace_plugins_root()
        .join(format!("provider-{id}"))
        .join("tests/fixtures")
        .join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

pub(super) fn load_inline_js(id: &str, plugin_js: &str) -> JsProvider {
    let manifest_json = format!(
        r#"{{"id":"{id}","version":"0.1.0","displayName":"Fixture",
                "spawn":{{"readinessStrategy":{{"kind":"fixed_delay","durationMs":0}},
                         "taskDelivery":"pty_input","verifySubmit":{{"kind":"none"}}}}}}"#
    );
    load_inline_js_with_manifest(id, &manifest_json, plugin_js)
}

pub(super) fn load_inline_js_with_manifest(
    id: &str,
    manifest_json: &str,
    plugin_js: &str,
) -> JsProvider {
    use std::sync::Arc;

    let manifest: crate::js_runtime::PluginManifest = serde_json::from_str(&manifest_json).unwrap();
    let vm = crate::js_runtime::PluginVm::from_source_with(
        id,
        plugin_js.to_string(),
        Some(manifest_json.to_string()),
    )
    .expect("fixture vm");
    JsProvider::new(manifest, Arc::new(vm))
}
