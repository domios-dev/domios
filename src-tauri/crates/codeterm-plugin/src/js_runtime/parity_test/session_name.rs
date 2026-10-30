use super::*;

#[test]
fn session_name_capability_native_js_parity() {
    let native = DiscoverModelsNativeStub;
    let js = load_inline_js(
        "session-name",
        "module.exports.default = { extractSessionName() { return null; } };",
    );
    assert_eq!(
        native.supports_session_name_detection(),
        js.supports_session_name_detection()
    );
    assert_eq!(
        native.extract_session_name("opaque"),
        js.extract_session_name("opaque")
    );
    js.vm()
        .reload("module.exports.default = {};".into())
        .unwrap();
    assert!(!js.supports_session_name_detection());
    assert_eq!(js.auth_status_probe(), None);
}

#[tokio::test]
async fn model_discovery_fingerprint_matches_native_observation_on_both_entry_points() {
    let binary = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(binary.path(), "opaque binary fixture").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(binary.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let mut manifest: super::super::PluginManifest = serde_json::from_value(serde_json::json!({
        "id": "discovery-fixture", "version": "1.0.0", "displayName": "Fixture",
        "cliBinary": binary.path(),
        "spawn": { "readinessStrategy": { "kind": "fixed_delay", "durationMs": 0 }, "taskDelivery": "pty_input", "verifySubmit": { "kind": "none" } }
    })).unwrap();
    manifest.binary_name = None;
    let raw = serde_json::to_string(&manifest).unwrap();
    let denied = super::super::PluginVm::from_source_sandboxed(
        &manifest.id,
        "module.exports.default = {};".into(),
        Some(raw.clone()),
        std::sync::Arc::new(
            crate::js_runtime::host_api::PluginSandbox::restricted(vec![], vec![])
                .with_identity(&manifest.id),
        ),
    )
    .unwrap();
    assert_eq!(denied.binary_fingerprint(), None);
    let source = r#"module.exports.default = {
        discoverModels(fingerprint) { return [{ id: fingerprint, displayName: 'opaque' }]; },
        models(fingerprint) { return [{ id: fingerprint, display_name: 'opaque' }]; }
    };"#;
    let vm = std::sync::Arc::new(
        super::super::PluginVm::from_source_with("discovery-fixture", source.into(), Some(raw))
            .unwrap(),
    );
    let provider = JsProvider::new(manifest, vm.clone());
    let expected = crate::model_catalog::binary_fingerprint_path(binary.path()).unwrap();
    assert_eq!(provider.discover_models().await.unwrap()[0].id, expected);
    let decision = super::super::JsDecisionModel::new(
        vm,
        "discovery-fixture",
        codeterm_decision::TokenMode::None,
    );
    assert_eq!(
        codeterm_decision::DecisionModel::models(&decision)[0].id,
        expected
    );
}
