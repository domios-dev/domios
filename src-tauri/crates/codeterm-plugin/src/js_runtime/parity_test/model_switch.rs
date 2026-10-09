use super::*;

#[path = "session_model_identity.rs"]
mod session_model_identity;

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
fn parity_codex_next_turn_model_switch() {
    let Some(_) = require_plugins() else {
        return;
    };
    let native = crate::provider::ModelSwitchMode::NextTurn;
    let js = load_js("codex").model_switch();

    assert_eq!(js, native);
    assert_eq!(
        serde_json::to_value(js).unwrap(),
        serde_json::json!("next_turn")
    );
}
