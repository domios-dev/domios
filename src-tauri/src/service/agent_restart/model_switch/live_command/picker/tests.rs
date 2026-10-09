use super::*;

fn commands() -> crate::agent::provider::ModelSwitchCommands {
    let manifest: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../plugins/provider-claude/plugin.json"
    )))
    .unwrap();
    serde_json::from_value(manifest["commands"]["modelSwitchCommands"].clone()).unwrap()
}

fn column_picker() -> crate::agent::provider::ModelSwitchPicker {
    use crate::agent::provider::{ModelSwitchPicker, ModelSwitchPickerPosition};
    ModelSwitchPicker {
        screen_regex: r"(?m)^\s*Effort\s*$[\s\S]*^\s*←/→ to adjust · Enter to confirm · s for this session only · Esc to cancel\s*\z".into(),
        selected_regex: r"(?m)^(?P<position>[^\r\n▲]*)▲[^\r\n]*\r?$".into(),
        target_regex: r"(?im)^(?P<position>[^\r\n]*?)(?P<label>\b{effort}\b)[^\r\n]*\s+Tab to toggle\s*\r?$".into(),
        position: ModelSwitchPickerPosition::Column,
        previous_key: "\x1b[D".into(),
        next_key: "\x1b[C".into(),
        response_key: "s".into(),
        cancel_key: "\x1b".into(),
    }
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
        display_name: "Opus 5.5".into(),
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
fn combined_switch_selects_current_sonnet_label_across_reordered_and_stale_frames() {
    let declaration = commands().model.picker.unwrap();
    let mut selection = target();
    selection.model = "sonnet".into();
    selection.display_name = "Sonnet 5.5".into();
    let mut picker = CommandPicker::new(&declaration, &selection, None).unwrap();
    picker.injected();
    let initial = MODEL_SCREEN
        .replace("❯ 4.", "  4.")
        .replace("    2.", "  ❯ 2.");
    let mut keys = Vec::new();
    picker
        .respond(&initial, |key| {
            keys.push(key.to_vec());
            Ok(())
        })
        .unwrap();
    let middle = MODEL_SCREEN
        .replace("❯ 4.", "  4.")
        .replace("    3.", "  ❯ 3.");
    picker
        .respond(&format!("{initial}\n{middle}"), |key| {
            keys.push(key.to_vec());
            Ok(())
        })
        .unwrap();
    picker
        .respond(&format!("{initial}\n{middle}\n{MODEL_SCREEN}"), |key| {
            keys.push(key.to_vec());
            Ok(())
        })
        .unwrap();
    assert_eq!(
        keys,
        [b"\x1b[B".to_vec(), b"\x1b[B".to_vec(), b"s".to_vec()]
    );
    let mut reordered = CommandPicker::new(&declaration, &selection, None).unwrap();
    reordered.injected();
    let screen = MODEL_SCREEN
        .replace("❯ 4.", "❯ 8.")
        .replace("    7.", "    4.");
    reordered
        .respond(&screen, |key| {
            assert_eq!(key, b"s");
            Ok(())
        })
        .unwrap();
}

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
fn column_picker_uses_the_current_gauge_and_ignores_history() {
    let mut picker = CommandPicker::new(&column_picker(), &target(), Some("xhigh")).unwrap();
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
fn already_selected_column_picker_uses_s_without_changing_the_model() {
    for screen in [
        EFFORT_SCREEN.replace("\r\n", "\n"),
        EFFORT_SCREEN.replace("\r\n", "\n").replace('\n', "\r\n"),
    ] {
        let mut picker = CommandPicker::new(&column_picker(), &target(), Some("high")).unwrap();
        picker.injected();
        let mut keys = Vec::new();
        picker
            .respond(&screen, |key| {
                keys.push(key.to_vec());
                Ok(())
            })
            .unwrap();
        assert_eq!(keys, vec![b"s".to_vec()]);
    }
}

#[test]
fn catalogue_labels_and_model_ids_match_lf_and_crlf_rows() {
    let declaration = commands().model.picker.unwrap();
    for screen in [
        MODEL_SCREEN.replace("\r\n", "\n"),
        MODEL_SCREEN.replace("\r\n", "\n").replace('\n', "\r\n"),
    ] {
        for (model, display_name) in [
            ("opus", "Opus 5.5"),
            ("opaque-id", "Opus 5.5"),
            ("opus", "Custom label"),
        ] {
            let mut selection = target();
            selection.model = model.into();
            selection.display_name = display_name.into();
            let mut picker = CommandPicker::new(&declaration, &selection, None).unwrap();
            picker.injected();
            let mut keys = Vec::new();
            picker
                .respond(&screen, |key| {
                    keys.push(key.to_vec());
                    Ok(())
                })
                .unwrap();
            assert_eq!(keys, vec![b"\x1b[A".to_vec()], "{model} / {display_name}");
        }
    }
}

#[test]
fn direct_effort_declaration_renders_target_and_confirms_captured_stdout() {
    let commands = commands();
    let plan = planned_commands(&commands, false, Some("high"), Some("medium"));
    assert_eq!(plan.len(), 1);
    assert_eq!(
        render_command(&plan[0].0.command, "haiku", plan[0].1).unwrap(),
        "/effort high"
    );
    assert!(plan[0].0.picker.is_none());
    assert!(plan[0].0.prompt.is_none());
    let pattern =
        confirmation_pattern(&plan[0].0.confirmation, "haiku", "Haiku 5.5", plan[0].1).unwrap();
    let captured = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../plugins/provider-claude/tests/fixtures/model-switch/effort-direct-confirmation.txt"
    ));
    assert!(captured
        .lines()
        .filter(|line| line.starts_with("Set effort level to high"))
        .any(|line| pattern.is_match(line)));
    assert!(!captured
        .lines()
        .filter(|line| line.starts_with("Set effort level to medium"))
        .any(|line| pattern.is_match(line)));
}
