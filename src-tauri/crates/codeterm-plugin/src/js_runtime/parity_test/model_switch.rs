use super::*;

#[path = "session_model_identity.rs"]
mod session_model_identity;

#[test]
fn runtime_model_switch_uses_the_declared_restart_resume_transport() {
    let Some(_) = require_plugins() else {
        return;
    };
    let js = load_js("opencode").model_switch();
    assert_eq!(js, crate::provider::ModelSwitchMode::RestartResume);
    assert_eq!(
        serde_json::to_value(js).unwrap(),
        serde_json::json!("restart_resume")
    );
}

#[test]
fn parity_live_model_switch() {
    let Some(_) = require_plugins() else {
        return;
    };
    let native = crate::provider::ModelSwitchMode::Live;
    let js = load_js("codex").model_switch();

    assert_eq!(js, native);
    assert_eq!(serde_json::to_value(js).unwrap(), serde_json::json!("live"));
}
