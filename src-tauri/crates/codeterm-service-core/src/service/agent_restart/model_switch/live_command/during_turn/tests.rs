use super::*;

fn fixture(relative: &str) -> String {
    super::super::tests::provider_file(relative)
}

fn declarations() -> (CommandPicker, CommandPrompt) {
    let manifest: serde_json::Value = serde_json::from_str(&fixture("plugin.json")).unwrap();
    let commands: crate::agent::provider::ModelSwitchCommands =
        serde_json::from_value(manifest["commands"]["modelSwitchCommands"].clone()).unwrap();
    let mut target = CommandTarget {
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
    let manifest_path = super::super::tests::provider_path("plugin.json");
    let provider =
        std::sync::Arc::new(crate::plugin::js_runtime::load_one(&manifest_path).unwrap());
    target.readiness =
        readiness::CommandReadiness::from_declaration(&commands, &target, provider).unwrap();
    (
        CommandPicker::new(commands.model.picker.as_ref().unwrap(), &target, None).unwrap(),
        CommandPrompt::new(commands.model.prompt.as_ref().unwrap(), &target, None)
            .unwrap()
            .with_structural_confirmation(target.readiness.clone(), "sonnet"),
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

#[test]
fn captured_picker_waits_for_its_footer_before_navigation() {
    let (mut picker, prompt) = declarations();
    picker.injected();
    let screen = fixture("tests/fixtures/model-switch/live-2.1.318-picker.txt");
    let partial = screen.split("  Enter to set as default").next().unwrap();
    assert_eq!(
        screen_stage(partial, Some(&picker), Some(&prompt), true),
        ScreenStage::AwaitingEvidence
    );
    picker
        .respond(partial, |_| panic!("partial picker must not write"))
        .unwrap();
    assert_eq!(
        screen_stage(&screen, Some(&picker), Some(&prompt), true),
        ScreenStage::Picker
    );
    let mut keys = Vec::new();
    picker
        .respond(&screen, |key| {
            keys.push(key.to_vec());
            Ok(())
        })
        .unwrap();
    assert_eq!(keys, [b"\x1b[A".to_vec()]);
}

#[test]
fn captured_footerless_confirmation_uses_provider_structure_and_cleans_both_dialogs() {
    let (picker, mut prompt) = declarations();
    let confirmation = fixture("tests/fixtures/model-switch/live-2.1.318-confirmation.txt");
    let picker_screen = fixture("tests/fixtures/model-switch/live-2.1.318-picker.txt");
    assert_eq!(
        screen_stage(&confirmation, Some(&picker), Some(&prompt), true),
        ScreenStage::Confirmation
    );
    for foreign in [
        confirmation.replace("Sonnet 5.5", "foreign"),
        format!("{confirmation}\n❯ "),
    ] {
        assert!(!prompt.present(&foreign));
    }
    prompt.injected();
    let mut keys = Vec::new();
    prompt
        .respond(&confirmation, |key| {
            keys.push(key.to_vec());
            Ok(())
        })
        .unwrap();
    prompt
        .respond(&confirmation, |_| panic!("confirmation cannot repeat"))
        .unwrap();
    assert_eq!(keys, [b"\r".to_vec()]);

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
    let mut cancel_picker = picker.cancellation_prompt(&target).unwrap();
    cancel_picker.injected();
    assert!(cancel_picker
        .cancel(&confirmation, |_| panic!("confirmation is not the picker"))
        .unwrap());
    keys.clear();
    assert!(!prompt
        .cancel(&confirmation, |key| {
            keys.push(key.to_vec());
            Ok(())
        })
        .unwrap());
    assert!(prompt
        .cancel(&picker_screen, |_| panic!("confirmation cleared"))
        .unwrap());
    assert!(!cancel_picker
        .cancel(&picker_screen, |key| {
            keys.push(key.to_vec());
            Ok(())
        })
        .unwrap());
    assert!(cancel_picker
        .cancel("❯ ", |_| panic!("picker cleared"))
        .unwrap());
    assert_eq!(keys, [b"\x1b".to_vec(), b"\x1b".to_vec()]);
}
