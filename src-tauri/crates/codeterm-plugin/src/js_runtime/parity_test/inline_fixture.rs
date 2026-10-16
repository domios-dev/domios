use super::*;

pub(super) fn load_inline_js(id: &str, plugin_js: &str) -> JsProvider {
    use std::sync::Arc;

    let manifest_json = format!(
        r#"{{"id":"{id}","version":"0.1.0","displayName":"Fixture",
                "spawn":{{"readinessStrategy":{{"kind":"fixed_delay","durationMs":0}},
                         "taskDelivery":"pty_input","verifySubmit":{{"kind":"none"}}}}}}"#
    );
    let manifest: crate::js_runtime::PluginManifest = serde_json::from_str(&manifest_json).unwrap();
    let vm = crate::js_runtime::PluginVm::from_source_with(
        id,
        plugin_js.to_string(),
        Some(manifest_json),
    )
    .expect("fixture vm");
    JsProvider::new(manifest, Arc::new(vm))
}
