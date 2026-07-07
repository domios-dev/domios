use super::*;
use crate::activity::PluginActivityRegistry;

#[test]
fn backend_defaults_to_claude_and_empty_model() {
    let (p, m) = backend_provider_model(&None);
    assert_eq!(p, "claude");
    assert_eq!(m, "");
}

#[test]
fn backend_honours_explicit_provider_and_model() {
    let b = Some(Backend {
        provider: Some("codex".to_string()),
        model: Some("gpt-5".to_string()),
    });
    let (p, m) = backend_provider_model(&b);
    assert_eq!(p, "codex");
    assert_eq!(m, "gpt-5");
}

#[test]
fn agent_resume_echoes_session_id() {
    let v: serde_json::Value = serde_json::from_str(&agent_resume_json("sess-7")).unwrap();
    assert_eq!(v["sessionId"], "sess-7");
}

#[test]
fn agent_get_is_null_host_keeps_no_key_map() {
    assert_eq!(agent_get_json(), "null");
}

#[test]
fn agent_usage_is_null_until_wired() {
    assert_eq!(agent_usage_json(), "null");
}

#[test]
fn progress_raw_starts_steps_and_finishes_plugin_activity() {
    let registry = PluginActivityRegistry::new();
    let mut current_id = None;

    let started = apply_progress_raw(
        &registry,
        &mut current_id,
        "transcriber",
        r#"{"label":"Installing ffmpeg"}"#,
    )
    .unwrap();

    assert_eq!(started.plugin_id, "transcriber");
    assert_eq!(started.kind, "bootstrap");
    assert_eq!(started.label, "Installing ffmpeg");
    assert_eq!(started.status, "running");
    assert_eq!(current_id, Some(started.id.clone()));

    let stepped = apply_progress_raw(
        &registry,
        &mut current_id,
        "transcriber",
        r#"{"label":"Downloading model","percent":42.5}"#,
    )
    .unwrap();

    assert_eq!(stepped.id, started.id);
    assert_eq!(stepped.label, "Downloading model");
    assert_eq!(stepped.percent, Some(42.5));
    assert_eq!(stepped.status, "running");
    assert_eq!(current_id, Some(started.id.clone()));

    let finished = apply_progress_raw(
        &registry,
        &mut current_id,
        "transcriber",
        r#"{"label":"Done","done":true}"#,
    )
    .unwrap();

    assert_eq!(finished.id, started.id);
    assert_eq!(finished.status, "done");
    assert_eq!(finished.error, None);
    assert_eq!(current_id, None);
}
