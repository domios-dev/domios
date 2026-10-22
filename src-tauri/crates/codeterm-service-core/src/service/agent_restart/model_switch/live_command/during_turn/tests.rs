use super::*;

fn fixture(relative: &str) -> String {
    super::super::tests::provider_file(relative)
}

fn declarations() -> (CommandPicker, CommandPrompt) {
    let manifest: serde_json::Value = serde_json::from_str(&fixture("plugin.json")).unwrap();
    let commands: crate::agent::provider::ModelSwitchCommands =
        serde_json::from_value(manifest["commands"]["modelSwitchCommands"].clone()).unwrap();
    let target = CommandTarget {
        tab_id: "owned".into(),
        pty_id: "pty".into(),
        provider: "opaque".into(),
        session_id: "session".into(),
        binding_nonce: "nonce".into(),
        cwd: None,
        model: "sonnet".into(),
        display_name: "Sonnet 5.5".into(),
        readiness: None,
    };
    (
        CommandPicker::new(commands.model.picker.as_ref().unwrap(), &target, None).unwrap(),
        CommandPrompt::new(commands.model.prompt.as_ref().unwrap(), &target, None).unwrap(),
    )
}

#[test]
fn recorded_mid_turn_frames_authorize_only_their_declared_keys_once() {
    let (mut picker, mut prompt) = declarations();
    picker.injected();
    prompt.injected();
    let mut keys = Vec::new();
    let busy = fixture("tests/fixtures/tab-state/normal-working.txt");
    let picker_screen = fixture("tests/fixtures/model-switch/model-picker.txt");
    let confirmation = fixture("tests/fixtures/model-switch/busy-sonnet-confirmation.txt");
    for screen in [
        busy.as_str(),
        &picker_screen,
        &picker_screen,
        &confirmation,
        &confirmation,
        &busy,
    ] {
        match screen_stage(screen, Some(&picker), Some(&prompt), false) {
            ScreenStage::Picker => picker
                .respond(screen, |key| {
                    keys.push(key.to_vec());
                    Ok(())
                })
                .unwrap(),
            ScreenStage::Confirmation => prompt
                .respond(screen, |key| {
                    keys.push(key.to_vec());
                    Ok(())
                })
                .unwrap(),
            ScreenStage::AwaitingEvidence => {}
            ScreenStage::UnknownPrompt | ScreenStage::Admission => panic!("unexpected prompt"),
        }
    }
    assert_eq!(keys, vec![b"s".to_vec(), b"\r".to_vec()]);
    assert_eq!(
        screen_stage(&busy, Some(&picker), Some(&prompt), false),
        ScreenStage::AwaitingEvidence
    );
    assert_eq!(
        screen_stage("foreign prompt", Some(&picker), Some(&prompt), true),
        ScreenStage::UnknownPrompt
    );
}

#[test]
fn confirmation_in_history_and_unrelated_selection_never_authorize_keys() {
    let (_, mut prompt) = declarations();
    prompt.injected();
    let confirmation = fixture("tests/fixtures/model-switch/sonnet-confirmation.txt");
    for screen in [
        format!("{confirmation}\n❯ "),
        confirmation.replace("Sonnet 5.5", "foreign"),
    ] {
        prompt
            .respond(&screen, |_| panic!("undeclared frame must not write"))
            .unwrap();
    }
    assert!(!prompt.answered());
}

#[test]
fn picker_with_normal_composer_below_it_navigates_and_cancels_structurally() {
    let (mut picker, _) = declarations();
    picker.injected();
    let screen = fixture("tests/fixtures/model-switch/model-picker-with-composer.txt");
    assert_eq!(
        screen_stage(&screen, Some(&picker), None, false),
        ScreenStage::Picker
    );
    let mut keys = Vec::new();
    picker
        .respond(&screen, |key| {
            keys.push(key.to_vec());
            Ok(())
        })
        .unwrap();
    let selected = screen.replace("❯ 5.", "  5.").replace("    4.", "  ❯ 4.");
    picker
        .respond(&selected, |key| {
            keys.push(key.to_vec());
            Ok(())
        })
        .unwrap();
    assert_eq!(keys, [b"\x1b[A".to_vec(), b"s".to_vec()]);
}
