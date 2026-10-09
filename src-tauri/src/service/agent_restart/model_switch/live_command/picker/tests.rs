use super::*;

fn commands() -> crate::agent::provider::ModelSwitchCommands {
    let manifest: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../plugins/provider-claude/plugin.json"
    )))
    .unwrap();
    serde_json::from_value(manifest["commands"]["modelSwitchCommands"].clone()).unwrap()
}

fn target() -> CommandTarget {
    CommandTarget {
        tab_id: "owned-tab".into(),
        pty_id: "owned-pty".into(),
        provider: "opaque".into(),
        session_id: "owned-session".into(),
        binding_nonce: "owned-nonce".into(),
        cwd: None,
        model: "opus".into(),
        display_name: "Claude Opus".into(),
    }
}

const MODEL_SCREEN: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../plugins/provider-claude/tests/fixtures/model-switch/model-picker.txt"
));
const EFFORT_SCREEN: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../plugins/provider-claude/tests/fixtures/model-switch/effort-picker.txt"
));

#[test]
fn picker_reobserves_each_cursor_before_session_only_submit() {
    let mut picker =
        CommandPicker::new(&commands().model.picker.unwrap(), &target(), None).unwrap();
    let mut keys = Vec::new();
    picker
        .respond(MODEL_SCREEN, |_| panic!("unowned picker"))
        .unwrap();
    picker.injected();
    picker
        .respond(MODEL_SCREEN, |key| {
            keys.push(key.to_vec());
            Ok(())
        })
        .unwrap();
    picker
        .respond(MODEL_SCREEN, |_| panic!("duplicate frame navigation"))
        .unwrap();
    let middle = MODEL_SCREEN.replace("❯ 4.", "  4.").replace("  3.", "❯ 3.");
    picker
        .respond(&middle, |key| {
            keys.push(key.to_vec());
            Ok(())
        })
        .unwrap();
    let final_frame = MODEL_SCREEN.replace("❯ 4.", "  4.").replace("  2.", "❯ 2.");
    picker
        .respond(&final_frame, |key| {
            keys.push(key.to_vec());
            Ok(())
        })
        .unwrap();
    picker
        .respond(&final_frame, |_| panic!("duplicate submit"))
        .unwrap();
    assert_eq!(
        keys,
        vec![b"\x1b[A".to_vec(), b"\x1b[A".to_vec(), b"s".to_vec()]
    );
}

#[test]
fn missing_model_refuses_without_navigation_or_submit() {
    let mut missing = target();
    missing.model = "absent".into();
    missing.display_name = "Absent model".into();
    let mut picker = CommandPicker::new(&commands().model.picker.unwrap(), &missing, None).unwrap();
    picker.injected();
    assert!(picker
        .respond(MODEL_SCREEN, |_| panic!("missing selection write"))
        .unwrap_err()
        .contains("requested selection is absent"));
}

#[test]
fn effort_uses_the_current_gauge_and_ignores_history() {
    let mut picker = CommandPicker::new(
        &commands().effort.unwrap().picker.unwrap(),
        &target(),
        Some("xhigh"),
    )
    .unwrap();
    picker.injected();
    let mut keys = Vec::new();
    picker
        .respond(EFFORT_SCREEN, |key| {
            keys.push(key.to_vec());
            Ok(())
        })
        .unwrap();
    picker
        .respond(EFFORT_SCREEN, |_| panic!("duplicate gauge navigation"))
        .unwrap();
    let next = EFFORT_SCREEN.replace(
        "────────────────────▲─────────────────────",
        "──────────────────────────────▲───────────",
    );
    picker
        .respond(&next, |key| {
            keys.push(key.to_vec());
            Ok(())
        })
        .unwrap();
    assert_eq!(keys, vec![b"\x1b[C".to_vec(), b"s".to_vec()]);
}

#[test]
fn already_selected_effort_uses_s_without_changing_the_model() {
    let mut picker = CommandPicker::new(
        &commands().effort.unwrap().picker.unwrap(),
        &target(),
        Some("high"),
    )
    .unwrap();
    picker.injected();
    picker
        .respond(EFFORT_SCREEN, |key| {
            assert_eq!(key, b"s");
            Ok(())
        })
        .unwrap();
}
