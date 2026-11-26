use super::*;

fn fixture(relative: &str) -> String {
    super::super::tests::provider_file(relative)
}

fn declarations() -> (CommandPicker, CommandPrompt) {
    declarations_with_label("Sonnet 5.5")
}

fn declarations_with_label(display_name: &str) -> (CommandPicker, CommandPrompt) {
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
        display_name: display_name.into(),
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
fn picker_checks_current_confirmation_before_waiting_for_another_frame() {
    let screens = codeterm_pty::pty::screen_state::ScreenState::default();
    let pty = "pty";
    screens.resize(pty, 120, 45);
    let signal = screens.subscribe_screen_changes(pty);
    let formatted = fixture("tests/fixtures/model-switch/eight-rc-5060e6f-confirmation.ansi");
    let plain = fixture("tests/fixtures/model-switch/eight-rc-5060e6f-confirmation.txt");
    let manifest_path = super::super::tests::provider_path("plugin.json");
    let provider = crate::plugin::js_runtime::load_one(&manifest_path).unwrap();
    let terminal_label =
        crate::agent::provider::AgentProvider::model_switch_terminal_label(&provider, "sonnet")
            .unwrap();
    let (mut picker, mut prompt) = declarations_with_label(&terminal_label);
    let (_, catalogue_prompt) = declarations_with_label(&format!("Catalogue {terminal_label}"));
    let picker_screen = fixture("tests/fixtures/model-switch/live-2.1.327-picker.txt");
    let selected = picker_screen
        .replace("❯ 5.", "  5.")
        .replace("    4.", "  ❯ 4.");
    let writes = std::cell::RefCell::new(Vec::new());
    picker.injected();
    prompt.injected();
    picker
        .respond(&picker_screen, |key| {
            writes.borrow_mut().push(key.to_vec());
            Ok(())
        })
        .unwrap();
    let wait_generation = std::cell::Cell::new(None);
    respond_to_dialogs(
        Some(&mut picker),
        Some(&mut prompt),
        |picker| {
            picker.respond(&selected, |key| {
                writes.borrow_mut().push(key.to_vec());
                screens.process(pty, formatted.as_bytes());
                wait_generation.set(Some(signal.generation()));
                Ok(())
            })
        },
        |prompt| {
            let screen = screens.get_screen_text(pty).unwrap();
            assert!(screen.contains("❯ 1. Yes, switch to Sonnet 5.5"));
            assert!(!catalogue_prompt.present(&plain));
            assert!(!catalogue_prompt.present(&screen));
            assert!(prompt.present(&plain));
            assert_eq!(wait_generation.get(), Some(signal.generation()));
            prompt.respond(&screen, |key| {
                writes.borrow_mut().push(key.to_vec());
                Ok(())
            })
        },
    )
    .unwrap();
    assert!(prompt.answered());
    respond_to_dialogs(
        Some(&mut picker),
        Some(&mut prompt),
        |_| panic!("picker cannot repeat"),
        |_| panic!("confirmation cannot repeat"),
    )
    .unwrap();
    assert_eq!(
        writes.into_inner(),
        [b"\x1b[A".to_vec(), b"s".to_vec(), b"\r".to_vec()]
    );
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
            ScreenStage::AwaitingEvidence | ScreenStage::PickerConfirmationPending => {}
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
    for (file, footer_prefix) in [
        (
            "tests/fixtures/model-switch/live-2.1.318-picker.txt",
            "  Enter to set as default",
        ),
        (
            "tests/fixtures/model-switch/live-2.1.332-picker.txt",
            "  s to use this session only",
        ),
    ] {
        let (mut picker, prompt) = declarations();
        picker.injected();
        let screen = fixture(file);
        let partial = screen.split(footer_prefix).next().unwrap();
        assert_eq!(
            screen_stage(partial, Some(&picker), Some(&prompt), true),
            ScreenStage::AwaitingEvidence,
            "{file}"
        );
        picker
            .respond(partial, |_| panic!("partial picker must not write"))
            .unwrap();
        assert_eq!(
            screen_stage(&screen, Some(&picker), Some(&prompt), true),
            ScreenStage::Picker,
            "{file}"
        );
        let mut keys = Vec::new();
        picker
            .respond(&screen, |key| {
                keys.push(key.to_vec());
                Ok(())
            })
            .unwrap();
        assert_eq!(keys, [b"\x1b[A".to_vec()], "{file}");
    }
}

#[test]
fn idle_tab_failure_on_during_turn_provider_is_not_reported_as_during_turn_incident() {
    let idle = CommandTurnState::resolve(true, false, false);
    assert!(!idle.buffering);
    assert!(!idle.report_incident);
    assert!(matches!(
        command_failure_error(
            idle,
            "model:AwaitingEvidence".into(),
            "captured screen".into(),
            false,
            "deadline expired".into(),
        ),
        ModelSwitchError::LiveCommandFailed(reason) if reason == "deadline expired"
    ));
    assert!(matches!(
        command_failure_error(
            idle,
            "model:Admission".into(),
            "captured screen".into(),
            true,
            "not ready".into(),
        ),
        ModelSwitchError::InputNotReady(reason) if reason == "not ready"
    ));

    for turn in [
        CommandTurnState::resolve(true, true, true),
        CommandTurnState::resolve(true, true, false),
        CommandTurnState::resolve(true, false, true),
    ] {
        assert!(turn.report_incident);
        assert!(matches!(
            command_failure_error(
                turn,
                "model:AwaitingEvidence".into(),
                "captured screen".into(),
                false,
                "deadline expired".into(),
            ),
            ModelSwitchError::DuringTurnIncident { stage, screen, reason }
                if stage == "model:AwaitingEvidence"
                    && screen == "captured screen"
                    && reason == "deadline expired"
        ));
    }

    let undeclared = CommandTurnState::resolve(false, true, true);
    assert!(!undeclared.buffering);
    assert!(!undeclared.report_incident);
    assert!(matches!(
        command_failure_error(
            undeclared,
            "model:AwaitingEvidence".into(),
            "captured screen".into(),
            false,
            "deadline expired".into(),
        ),
        ModelSwitchError::LiveCommandFailed(reason) if reason == "deadline expired"
    ));
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

#[test]
fn cached_model_dialog_with_confirmation_footer_is_answered_once_after_picker() {
    for frame in [
        "live-2.1.327-confirmation-90.txt",
        "live-2.1.327-confirmation-120.txt",
        "eight-rc-2.1.327-confirmation.txt",
    ] {
        let (mut picker, mut prompt) = declarations();
        let screen = fixture("tests/fixtures/model-switch/live-2.1.327-picker.txt");
        let confirmation = fixture(&format!("tests/fixtures/model-switch/{frame}"));
        let partial = confirmation.split("  ❯ 1.").next().unwrap();
        let selected = screen.replace("❯ 5.", "  5.").replace("    4.", "  ❯ 4.");
        picker.injected();
        prompt.injected();
        let mut keys = Vec::new();

        assert_eq!(
            screen_stage(partial, Some(&picker), Some(&prompt), true),
            ScreenStage::UnknownPrompt
        );
        for (screen, expected) in [
            (screen.as_str(), ScreenStage::Picker),
            (&selected, ScreenStage::Picker),
            (partial, ScreenStage::PickerConfirmationPending),
            ("Unrelated prompt", ScreenStage::PickerConfirmationPending),
            (&confirmation, ScreenStage::Confirmation),
            (&confirmation, ScreenStage::Confirmation),
        ] {
            let mut write = |key: &[u8]| {
                keys.push(key.to_vec());
                Ok(())
            };
            let stage = screen_stage(screen, Some(&picker), Some(&prompt), true);
            assert_eq!(stage, expected, "{frame}");
            match stage {
                ScreenStage::Picker => picker.respond(screen, &mut write).unwrap(),
                ScreenStage::Confirmation => prompt.respond(screen, write).unwrap(),
                ScreenStage::PickerConfirmationPending => prompt
                    .respond(screen, |_| panic!("unmatched frame must not write"))
                    .unwrap(),
                stage => panic!("unexpected switch stage {stage:?}"),
            }
        }
        assert!(picker.submitted());
        assert!(prompt.answered());
        assert_eq!(
            screen_stage("Unrelated prompt", Some(&picker), Some(&prompt), true),
            ScreenStage::UnknownPrompt
        );
        assert_eq!(keys, [b"\x1b[A".to_vec(), b"s".to_vec(), b"\r".to_vec()]);
        assert!(!prompt.present(
            &confirmation.replace("Yes, switch to Sonnet 5.5", "Yes, switch to Haiku 5.5")
        ));
    }
}
