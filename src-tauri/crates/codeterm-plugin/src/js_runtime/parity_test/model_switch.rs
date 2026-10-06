use super::*;

#[test]
fn opencode_live_model_switch_matches_the_native_capability_wire_value() {
    let Some(_) = require_plugins() else {
        return;
    };
    let native = crate::provider::ModelSwitchMode::Live;
    let js = load_js("opencode").model_switch();

    assert_eq!(
        serde_json::to_value(native).unwrap(),
        serde_json::json!("live")
    );
    assert_eq!(js, native);
    assert_eq!(
        serde_json::to_value(js).unwrap(),
        serde_json::to_value(native).unwrap()
    );
}

#[test]
fn codex_model_switch_remains_restart_resume() {
    let Some(_) = require_plugins() else {
        return;
    };
    let native = crate::provider::ModelSwitchMode::RestartResume;
    let js = load_js("codex").model_switch();

    assert_eq!(js, native);
    assert_eq!(
        serde_json::to_value(js).unwrap(),
        serde_json::json!("restart_resume")
    );
}
