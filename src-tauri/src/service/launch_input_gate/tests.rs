use super::*;
use crate::agent::provider::AgentProvider;
use crate::agent::test_provider::MockProvider;

#[test]
fn unsupported_codex_layout_fails_with_named_reason() {
    let refusal = launch_readiness_deadline_refusal(true, true);
    assert_eq!(refusal, LaunchInputRefusal::ComposerNotRecognised);
    assert_eq!(
        refusal.reason(),
        "provider composer layout was not recognised"
    );
}

#[test]
fn readiness_deadline_without_provider_tui_keeps_generic_reason() {
    assert_eq!(
        launch_readiness_deadline_refusal(true, false),
        LaunchInputRefusal::Deadline
    );
    assert_eq!(
        launch_readiness_deadline_refusal(false, true),
        LaunchInputRefusal::Deadline
    );
}

#[test]
fn provider_cursor_composer_matches_captured_bottom_region_fixture() {
    let mut screens = codeterm_pty::pty::screen_state::ScreenState::new();
    screens.resize("cursor", 120, 24);
    let text = include_str!(
        "../../../../plugins/provider-cursor/tests/fixtures/composer-readiness/cursor-ready.txt"
    )
    .lines()
    .collect::<Vec<_>>()
    .join("\r\n");
    screens.process("cursor", format!("\x1b[2J\x1b[H{text}\x1b[?25h").as_bytes());
    let rows = screens.get_screen_rows("cursor").expect("screen rows");
    let composer_row = rows
        .iter()
        .position(|line| line.trim_start().starts_with('→'))
        .expect("captured cursor composer") as u16;
    screens.process("cursor", format!("\x1b[{};1H", composer_row + 1).as_bytes());
    let input = screens
        .composer_input_state("cursor")
        .expect("cursor input state");
    let rows = screens.get_screen_rows("cursor").expect("screen rows");
    assert!(launch_composer_input_ready(
        &input,
        crate::agent::provider::ComposerCursorPosition::BottomRegion,
        Some(&rows),
        1,
    ));
}

fn launch_classification(state: TabState) -> Option<TabStateClassification> {
    Some(TabStateClassification {
        state,
        confidence: 1.0,
        data: None,
        action: None,
    })
}

fn input_observation() -> LaunchInputObservation {
    LaunchInputObservation {
        session_bound: false,
        composer_ready: true,
        pending_prompt: false,
        provider_process_alive: true,
        provider_busy: false,
        directory_trust_prompt: false,
        active_interactive_prompt: false,
        classification: None,
    }
}

#[test]
fn launch_gate_allows_unknown_provider_state_with_readiness_evidence() {
    assert_eq!(
        decide_launch_input(
            LaunchInputObservation {
                session_bound: true,
                composer_ready: false,
                classification: launch_classification(TabState::Unknown),
                ..input_observation()
            },
            LaunchInputMode::Standard,
        ),
        Ok(Some(LaunchInputEvidence::SessionBinding)),
    );
    assert_eq!(
        decide_launch_input(input_observation(), LaunchInputMode::Standard),
        Ok(Some(LaunchInputEvidence::ProviderComposer)),
    );
}

#[test]
fn launch_gate_refuses_provider_and_pending_prompt_dialogs() {
    for state in [
        TabState::UpdatePrompt,
        TabState::WaitingOnAuth,
        TabState::RateLimited,
        TabState::ClarifyingQuestion,
    ] {
        let refusal = decide_launch_input(
            LaunchInputObservation {
                classification: launch_classification(state),
                active_interactive_prompt: state == TabState::ClarifyingQuestion,
                ..input_observation()
            },
            LaunchInputMode::Standard,
        )
        .expect_err("classified provider dialog blocks launch input");
        assert_eq!(refusal, LaunchInputRefusal::ProviderDialog(state));
    }

    assert_eq!(
        decide_launch_input(
            LaunchInputObservation {
                pending_prompt: true,
                ..input_observation()
            },
            LaunchInputMode::LazySessionStart,
        ),
        Ok(None),
        "an active structured prompt blocks lazy readiness",
    );

    let refusal = decide_launch_input(
        LaunchInputObservation {
            directory_trust_prompt: true,
            ..input_observation()
        },
        LaunchInputMode::Standard,
    )
    .expect_err("provider directory-trust modal blocks launch input");
    assert_eq!(refusal, LaunchInputRefusal::DirectoryTrustPrompt);
}

#[test]
fn lazy_session_start_blocks_classified_modals_and_active_questions() {
    for state in [
        TabState::UpdatePrompt,
        TabState::WaitingOnAuth,
        TabState::RateLimited,
    ] {
        assert_eq!(
            decide_launch_input(
                LaunchInputObservation {
                    classification: launch_classification(state),
                    ..input_observation()
                },
                LaunchInputMode::LazySessionStart,
            ),
            Ok(None),
        );
    }

    assert_eq!(
        decide_launch_input(
            LaunchInputObservation {
                classification: launch_classification(TabState::ClarifyingQuestion),
                active_interactive_prompt: true,
                ..input_observation()
            },
            LaunchInputMode::LazySessionStart,
        ),
        Ok(None),
    );
}

#[test]
fn resumed_transcript_question_does_not_block_without_an_active_structured_prompt() {
    let transcript = "Earlier response text.\nassistant: What would you like me to do next?\n";
    let transcript_provider = MockProvider::new("transcript-question")
        .with_tab_state(launch_classification(TabState::ClarifyingQuestion).unwrap());
    let classification = transcript_provider.classify_tab_state(&TabStateSnapshot {
        tab_id: "restored-tab".into(),
        agent_type: "fixture".into(),
        session_id: None,
        cwd: None,
        screen_text: transcript.into(),
    });
    let active_interactive_prompt = classification.as_ref().is_some_and(|value| {
        value.state == TabState::ClarifyingQuestion
            && value.confidence >= crate::agent::state_reactor::GLOBAL_THRESHOLD
    }) && transcript_provider.parse_prompt(transcript).is_some();
    assert!(!active_interactive_prompt);

    assert_eq!(
        decide_launch_input(
            LaunchInputObservation {
                session_bound: true,
                composer_ready: false,
                pending_prompt: true,
                classification: classification.clone(),
                active_interactive_prompt,
                ..input_observation()
            },
            LaunchInputMode::Standard,
        ),
        Ok(Some(LaunchInputEvidence::SessionBinding)),
    );
    assert_eq!(
        decide_launch_input(
            LaunchInputObservation {
                classification,
                active_interactive_prompt,
                ..input_observation()
            },
            LaunchInputMode::LazySessionStart,
        ),
        Ok(Some(LaunchInputEvidence::ProviderComposer)),
    );

    let active_prompt = codeterm_types::pty::PendingPrompt {
        question: "Active structured choice".into(),
        options: vec![
            codeterm_types::pty::PromptOption {
                label: "Option one".into(),
                value: "1".into(),
            },
            codeterm_types::pty::PromptOption {
                label: "Option two".into(),
                value: "2".into(),
            },
        ],
        sections: None,
        multi_select: Some(false),
        submit_available: Some(false),
        selected_values: Some(vec!["1".into()]),
    };
    let active_provider = MockProvider::new("active-question")
        .with_tab_state(launch_classification(TabState::ClarifyingQuestion).unwrap())
        .with_parsed_prompt(active_prompt);
    let active_screen = "active input region";
    let active_classification = active_provider.classify_tab_state(&TabStateSnapshot {
        tab_id: "restored-tab".into(),
        agent_type: "fixture".into(),
        session_id: None,
        cwd: None,
        screen_text: active_screen.into(),
    });
    let active_interactive_prompt = active_classification.as_ref().is_some_and(|value| {
        value.state == TabState::ClarifyingQuestion
            && value.confidence >= crate::agent::state_reactor::GLOBAL_THRESHOLD
    }) && active_provider.parse_prompt(active_screen).is_some();
    assert!(active_interactive_prompt);
    assert_eq!(
        decide_launch_input(
            LaunchInputObservation {
                classification: active_classification,
                active_interactive_prompt,
                ..input_observation()
            },
            LaunchInputMode::Standard,
        ),
        Err(LaunchInputRefusal::ProviderDialog(
            TabState::ClarifyingQuestion
        )),
    );
}

#[test]
fn lazy_session_start_requires_composer_live_process_and_no_prompt() {
    assert_eq!(
        decide_launch_input(input_observation(), LaunchInputMode::LazySessionStart),
        Ok(Some(LaunchInputEvidence::ProviderComposer)),
    );

    for blocked in [
        LaunchInputObservation {
            provider_process_alive: false,
            ..input_observation()
        },
        LaunchInputObservation {
            pending_prompt: true,
            ..input_observation()
        },
        LaunchInputObservation {
            directory_trust_prompt: true,
            ..input_observation()
        },
        LaunchInputObservation {
            classification: launch_classification(TabState::UpdatePrompt),
            ..input_observation()
        },
    ] {
        assert_eq!(
            decide_launch_input(blocked, LaunchInputMode::LazySessionStart),
            Ok(None),
            "blocked lazy launch remains unready until its bounded deadline",
        );
    }

    assert_eq!(
        decide_launch_input(
            LaunchInputObservation {
                composer_ready: false,
                session_bound: true,
                ..input_observation()
            },
            LaunchInputMode::LazySessionStart,
        ),
        Ok(None),
        "a live provider without its declared composer is not ready",
    );
}

#[test]
fn lazy_composer_readiness_requires_declared_structural_capability() {
    assert!(!provider_composer_is_ready(
        LaunchInputMode::LazySessionStart,
        true,
        true,
        false,
    ));
    assert!(provider_composer_is_ready(
        LaunchInputMode::LazySessionStart,
        true,
        true,
        true,
    ));
    assert!(!provider_composer_is_ready(
        LaunchInputMode::LazySessionStart,
        false,
        true,
        true,
    ));
    assert!(!provider_composer_is_ready(
        LaunchInputMode::LazySessionStart,
        true,
        false,
        true,
    ));
}

#[test]
fn opencode_sessionless_home_frame_accepts_centered_visible_composer() {
    let plugins = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../plugins"));
    let provider =
        crate::plugin::js_runtime::load_one(&plugins.join("provider-opencode").join("plugin.json"))
            .expect("load bundled OpenCode provider");
    let screen = std::fs::read_to_string(
        plugins
            .join("provider-opencode")
            .join("tests")
            .join("fixtures")
            .join("sessionless-home-screen.txt"),
    )
    .expect("read captured OpenCode TUI frame");
    let snapshot = TabStateSnapshot {
        tab_id: "opencode-sessionless-home".into(),
        agent_type: "opencode".into(),
        session_id: None,
        cwd: None,
        screen_text: screen.clone(),
    };
    let input = codeterm_pty::pty::screen_state::ComposerInputState {
        alternate_screen: true,
        cursor_visible: true,
        cursor_row: 20,
        rows: 40,
        cols: 120,
        inverse_caret_row: None,
        nonblank_inverse_cells: 0,
        sole_nonblank_inverse_row: None,
        last_nonblank_row: None,
    };
    let position = provider.composer_cursor_position();
    let tui_evidence = provider.screen_has_tui(&screen);
    let terminal_input_ready = launch_composer_input_ready(&input, position, None, 0);
    let classification = provider.classify_tab_state(&snapshot);
    let active_interactive_prompt = classification.as_ref().is_some_and(|value| {
        value.state == TabState::ClarifyingQuestion
            && value.confidence >= crate::agent::state_reactor::GLOBAL_THRESHOLD
    }) && provider.parse_prompt(&screen).is_some();

    assert!(
        tui_evidence,
        "the provider recognizes its captured TUI frame"
    );
    assert!(
        terminal_input_ready,
        "centered visible cursor is provider-declared"
    );
    assert!(snapshot.session_id.is_none());
    assert!(provider_composer_is_ready(
        LaunchInputMode::LazySessionStart,
        provider.provider_readiness_strategy().requires_composer(),
        terminal_input_ready,
        tui_evidence,
    ));
    assert_eq!(
        decide_launch_input(
            LaunchInputObservation {
                composer_ready: true,
                classification,
                active_interactive_prompt,
                ..input_observation()
            },
            LaunchInputMode::LazySessionStart,
        ),
        Ok(Some(LaunchInputEvidence::ProviderComposer)),
        "an unbound sessionless launch is ready from its composer",
    );
}

#[test]
fn centered_composer_readiness_rejects_busy_and_non_tui_frames() {
    let input_ready = launch_composer_input_ready(
        &codeterm_pty::pty::screen_state::ComposerInputState {
            alternate_screen: true,
            cursor_visible: true,
            cursor_row: 20,
            rows: 40,
            cols: 120,
            inverse_caret_row: None,
            nonblank_inverse_cells: 0,
            sole_nonblank_inverse_row: None,
            last_nonblank_row: None,
        },
        crate::agent::provider::ComposerCursorPosition::VisibleAnywhere,
        None,
        0,
    );
    assert!(input_ready);
    assert!(!provider_composer_is_ready(
        LaunchInputMode::LazySessionStart,
        true,
        input_ready,
        false,
    ));
    let working = launch_classification(TabState::Working);
    let provider_busy = provider_is_busy(
        LaunchInputMode::LazySessionStart,
        crate::agent::provider::ComposerCursorPosition::VisibleAnywhere,
        working.as_ref(),
    );
    assert!(provider_busy);
    assert_eq!(
        decide_launch_input(
            LaunchInputObservation {
                composer_ready: true,
                provider_busy,
                ..input_observation()
            },
            LaunchInputMode::LazySessionStart,
        ),
        Ok(None),
        "a provider-classified working frame stays blocked",
    );
    let busy_input_ready = launch_composer_input_ready(
        &codeterm_pty::pty::screen_state::ComposerInputState {
            alternate_screen: true,
            cursor_visible: false,
            cursor_row: 20,
            rows: 40,
            cols: 120,
            inverse_caret_row: None,
            nonblank_inverse_cells: 0,
            sole_nonblank_inverse_row: None,
            last_nonblank_row: None,
        },
        crate::agent::provider::ComposerCursorPosition::VisibleAnywhere,
        None,
        0,
    );
    assert!(!busy_input_ready);
    assert_eq!(
        decide_launch_input(
            LaunchInputObservation {
                composer_ready: busy_input_ready,
                ..input_observation()
            },
            LaunchInputMode::LazySessionStart,
        ),
        Ok(None),
    );
}

#[test]
fn centered_cursor_capability_does_not_change_bottom_composer_providers() {
    let input = codeterm_pty::pty::screen_state::ComposerInputState {
        alternate_screen: true,
        cursor_visible: true,
        cursor_row: 20,
        rows: 40,
        cols: 120,
        inverse_caret_row: None,
        nonblank_inverse_cells: 0,
        sole_nonblank_inverse_row: None,
        last_nonblank_row: None,
    };
    assert!(!launch_composer_input_ready(
        &input,
        crate::agent::provider::ComposerCursorPosition::BottomRegion,
        None,
        0
    ));
    assert!(!provider_composer_is_ready(
        LaunchInputMode::LazySessionStart,
        false,
        true,
        true,
    ));
}

#[test]
fn codex_update_and_folder_trust_frames_block_lazy_readiness() {
    let plugins = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../plugins"));
    let provider =
        crate::plugin::js_runtime::load_one(&plugins.join("provider-codex").join("plugin.json"))
            .expect("load the bundled Codex provider");
    let fixtures = plugins
        .join("provider-codex")
        .join("tests")
        .join("fixtures")
        .join("directory-trust");

    for file in [
        "codex-update-available.txt",
        "trust-folder-access-0.157.1.txt",
    ] {
        let screen = std::fs::read_to_string(fixtures.join(file)).expect("read captured screen");
        let classification = provider.classify_tab_state(&TabStateSnapshot {
            tab_id: "codex-launch-fixture".into(),
            agent_type: "codex".into(),
            session_id: None,
            cwd: None,
            screen_text: screen.clone(),
        });
        let directory_trust_prompt = provider.directory_trust_prompt_response(&screen).is_some();
        let active_interactive_prompt = classification.as_ref().is_some_and(|value| {
            value.state == TabState::ClarifyingQuestion
                && value.confidence >= crate::agent::state_reactor::GLOBAL_THRESHOLD
        }) && provider.parse_prompt(&screen).is_some();

        assert_eq!(
            decide_launch_input(
                LaunchInputObservation {
                    directory_trust_prompt,
                    active_interactive_prompt,
                    classification,
                    ..input_observation()
                },
                LaunchInputMode::LazySessionStart,
            ),
            Ok(None),
            "captured provider frame {file} must block lazy input readiness",
        );
    }
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn lazy_session_spawn_with_modal_or_dead_provider_still_fails_at_deadline() {
    fn await_blocked_launch(
        classification: Option<TabState>,
    ) -> Result<LaunchInputEvidence, LaunchInputRefusal> {
        let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let pty_id = "blocked-lazy-session-pty";
        let nonce = "blocked-lazy-session-nonce";
        {
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.agent_type = Some("blocked-lazy-session-fixture".into());
            tab.session_binding_nonce = Some(nonce.into());
            tab.pty_id = Some(pty_id.into());
        }
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        let provider = match classification {
            Some(state) => MockProvider::new("blocked-lazy-session-fixture").with_tab_state(
                TabStateClassification {
                    state,
                    confidence: 1.0,
                    data: None,
                    action: None,
                },
            ),
            None => MockProvider::new("blocked-lazy-session-fixture"),
        };
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(provider));
        let attempt = LaunchAttempt {
            tab_id,
            pty_id: pty_id.into(),
            binding_nonce: nonce.into(),
        };

        fixture.service.await_lazy_session_input_readiness(
            &attempt,
            "blocked-lazy-session-fixture",
            std::time::Duration::ZERO,
        )
    }

    assert_eq!(
        await_blocked_launch(Some(TabState::UpdatePrompt)),
        Err(LaunchInputRefusal::Deadline),
    );
    assert_eq!(
        await_blocked_launch(None),
        Err(LaunchInputRefusal::Deadline),
    );
}
