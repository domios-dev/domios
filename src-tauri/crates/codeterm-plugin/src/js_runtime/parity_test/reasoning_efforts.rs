use super::*;

#[tokio::test]
async fn per_model_reasoning_efforts_native_js_parity() {
    let native = DiscoverModelsNativeStub;
    let js = load_inline_js(
        "discover-fixture",
        r#"
            module.exports.default = {
                discoverModels() {
                    return [
                        { id: 'live-a', displayName: 'Live A', group: 'alpha', defaultReasoningEffort: 'max', reasoningEfforts: [
                            { id: 'max', displayName: 'Max', description: 'Maximum reasoning' },
                            { id: 'ultra', displayName: 'Ultra' }
                        ] },
                        { id: 'live-b', displayName: 'Live B', description: 'second' },
                        { id: 'live-c', displayName: 'Live C', availability: 'unknown' }
                    ];
                }
            };
        "#,
    );

    assert_eq!(js.discover_models().await, Some(discover_models_fixture()));
    assert_eq!(native.discover_models().await, js.discover_models().await);
}

#[test]
fn reasoning_effort_capability_is_declared_and_gated() {
    if require_plugins().is_none() {
        return;
    }

    let codex = load_js("codex");
    let d = codex.describe();
    let ids: Vec<String> = d.reasoning_efforts.iter().map(|e| e.id.clone()).collect();
    assert_eq!(
        ids,
        vec!["low", "medium", "high", "xhigh"],
        "codex must surface its declared effort levels"
    );
    assert_eq!(d.default_reasoning_effort.as_deref(), Some("medium"));
    assert_eq!(
        codex.reasoning_effort_launch_args("high"),
        vec![
            "-c".to_string(),
            "model_reasoning_effort=\"high\"".to_string()
        ],
        "codex effort launch flag must match its CLI config-override contract"
    );
    assert!(
        codex.reasoning_effort_launch_args("").is_empty(),
        "empty effort must produce no launch flags"
    );
    assert!(codex.supports_reasoning_effort("medium"));
    assert!(codex.validate_reasoning_effort(Some("high")).is_ok());
    let rejection = codex
        .validate_reasoning_effort(Some("ultra"))
        .expect_err("unsupported effort must be rejected");
    assert_eq!(rejection.requested, "ultra");
    assert_eq!(rejection.supported, vec!["low", "medium", "high", "xhigh"]);

    let claude = load_js("claude");
    let claude_ids: Vec<String> = claude
        .describe()
        .reasoning_efforts
        .iter()
        .map(|effort| effort.id.clone())
        .collect();
    assert_eq!(claude_ids, vec!["low", "medium", "high", "xhigh", "max"]);
    assert_eq!(
        claude.reasoning_effort_launch_args("xhigh"),
        vec!["--effort".to_string(), "xhigh".to_string()]
    );
    assert!(claude.validate_reasoning_effort(None).is_ok());
    assert!(claude.validate_reasoning_effort(Some("max")).is_ok());
    assert!(claude.validate_reasoning_effort(Some("ultra")).is_err());

    let native_claude = crate::agent_claude::provider::ClaudeProvider::new();
    assert_eq!(
        native_claude.reasoning_efforts(),
        claude.reasoning_efforts()
    );
    assert_eq!(
        native_claude.reasoning_effort_launch_args("high"),
        claude.reasoning_effort_launch_args("high")
    );
}

#[test]
fn session_owned_reasoning_effort_native_js_parity() {
    let native = DiscoverModelsNativeStub;
    let js = load_inline_js(
        "effort-attestation-fixture",
        r#"
            module.exports.default = {
                detectSessionReasoningEffort(_cwd, sessionId) {
                    return sessionId === 'owned-session' ? 'high' : null;
                }
            };
        "#,
    );
    for session_id in ["owned-session", "global-latest-foreign"] {
        assert_eq!(
            native.detect_session_reasoning_effort("/repo", session_id),
            js.detect_session_reasoning_effort("/repo", session_id),
        );
    }
}

#[test]
fn model_catalog_override_applies_reasoning_efforts() {
    use std::sync::Arc;

    let id = "model-catalog-effort-override-fixture";
    let manifest_json = format!(
        r#"{{"id":"{id}","version":"0.1.0","displayName":"Fixture",
                "models":[{{"id":"fixture-model","displayName":"Fixture Model"}}],
                "reasoningEfforts":[
                    {{"id":"low","displayName":"Low"}},
                    {{"id":"medium","displayName":"Medium"}},
                    {{"id":"high","displayName":"High"}}
                ],
                "defaultReasoningEffort":"medium",
                "spawn":{{"readinessStrategy":{{"kind":"fixed_delay","durationMs":0}},
                         "taskDelivery":"pty_input","verifySubmit":{{"kind":"none"}}}}}}"#
    );
    let manifest: super::super::PluginManifest = serde_json::from_str(&manifest_json).unwrap();
    let vm = super::super::PluginVm::from_source_with(
        id,
        "module.exports.default = {};".to_string(),
        Some(manifest_json),
    )
    .expect("fixture vm");
    let override_path = crate::model_catalog::models_dir().join(format!("{id}.toml"));
    std::fs::write(
        &override_path,
        r#"[[models]]
id = "fixture-model"
display_name = "Fixture Model"

[[efforts]]
id = "xhigh"
display_name = "Extra high"
description = "Maximum reasoning depth."
"#,
    )
    .unwrap();

    let provider = JsProvider::new(manifest, Arc::new(vm));
    let _ = provider.available_models();
    let efforts: Vec<String> = provider
        .reasoning_efforts()
        .into_iter()
        .map(|effort| effort.id)
        .collect();
    let validation = provider.validate_reasoning_effort(Some("xhigh"));

    let _ = std::fs::remove_file(&override_path);
    assert!(
        efforts.iter().any(|effort| effort == "xhigh"),
        "model-catalog override discarded [[efforts]]; resolved efforts: {efforts:?}"
    );
    assert!(validation.is_ok());
}
