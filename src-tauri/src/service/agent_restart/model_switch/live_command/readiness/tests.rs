use super::*;

fn declared() -> (Arc<CommandReadiness>, String, ComposerInputState) {
    let manifest: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../plugins/provider-claude/plugin.json"
    )))
    .unwrap();
    let commands =
        serde_json::from_value(manifest["commands"]["modelSwitchCommands"].clone()).unwrap();
    let target = CommandTarget {
        tab_id: "tab".into(),
        pty_id: "pty".into(),
        provider: "provider".into(),
        session_id: "session".into(),
        binding_nonce: "nonce".into(),
        cwd: None,
        model: "sonnet".into(),
        display_name: "Sonnet 5.5".into(),
        readiness: None,
    };
    let readiness = CommandReadiness::from_declaration(&commands, &target)
        .unwrap()
        .unwrap();
    let screen = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../plugins/provider-claude/tests/fixtures/model-switch/confirmed-ready-frame.txt"
    ))
    .to_owned();
    let row = screen.lines().position(|row| row.trim() == "❯").unwrap() as u16;
    let input = ComposerInputState {
        alternate_screen: false,
        cursor_visible: true,
        cursor_row: row,
        rows: screen.lines().count() as u16,
        cols: 120,
        inverse_caret_row: None,
        nonblank_inverse_cells: 0,
        sole_nonblank_inverse_row: None,
        last_nonblank_row: Some(row + 2),
    };
    (readiness, screen, input)
}

#[test]
fn confirmed_ready_frame_advances_without_status_idle() {
    let (readiness, screen, input) = declared();
    assert!(input_ready(
        Some(&readiness),
        Some((&screen, &input)),
        false
    ));
    assert!(!input_ready(Some(&readiness), None, true));
    assert!(input_ready(None, None, true));
    assert!(!input_ready(None, None, false));
}

#[test]
fn populated_composer_or_foreign_cursor_cannot_advance() {
    let (readiness, screen, mut input) = declared();
    assert!(!readiness.matches(&screen.replace("❯\u{a0}", "❯ another command"), &input));
    input.cursor_row = 0;
    assert!(!readiness.matches(&screen, &input));
    input.cursor_visible = false;
    assert!(!readiness.matches(&screen, &input));
}

#[test]
fn declared_dialog_blocks_empty_composer_and_inverse_caret_can_prove_readiness() {
    let (readiness, screen, mut input) = declared();
    input.cursor_visible = false;
    input.inverse_caret_row = Some(input.cursor_row);
    assert!(readiness.matches(&screen, &input));
    for dialog in [
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../plugins/provider-claude/tests/fixtures/model-switch/model-picker.txt")),
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../plugins/provider-claude/tests/fixtures/model-switch/cached-sonnet-confirmation.txt")),
    ] {
        assert!(!readiness.matches(&format!("{}\n{}", screen, dialog), &input));
    }
}
