use std::cell::Cell;

use codeterm_plugin::provider::{AgentProvider, DirectoryTrustPromptResponse};

use super::{
    decide, decide_with_frame_identity, directory_trust_check_applies,
    directory_trust_provenance, directory_trust_skip_reason,
    inherited_workspace_provenance, legacy_auto_approve_permitted, DirectoryTrustRefusal,
    TrustDecision, TrustLedger,
};
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{PendingPrompt, PromptOption};

fn state_with_tab(cwd: Option<&str>, provisioned: Option<&str>) -> (MultiplexerState, String) {
    let (mut state, _root) = MultiplexerState::new();
    let ws_id = state.list_workspaces()[0].id.clone();
    let tab_id = state.create_tab(&ws_id).expect("tab");
    let tab = state.get_tab_mut(&tab_id).expect("tab");
    tab.cwd = cwd.map(str::to_string);
    tab.provisioned_cwd = provisioned.map(str::to_string);
    (state, tab_id)
}

fn agent_tab(provisioned: Option<&str>, authenticated: bool) -> (MultiplexerState, String) {
    let (mut state, tab_id) = state_with_tab(Some("/srv/lanes/a"), provisioned);
    let tab = state.get_tab_mut(&tab_id).expect("tab");
    tab.agent_type = Some("claude".into());
    if authenticated {
        tab.agent_session_id = Some("session-1".into());
        tab.session_binding_nonce = Some("nonce-1".into());
        tab.reported_session_binding_nonce = Some("nonce-1".into());
    }
    (state, tab_id)
}

fn answer(step: &[u8]) -> Option<DirectoryTrustPromptResponse> {
    Some(DirectoryTrustPromptResponse::Answer {
        step: step.to_vec(),
    })
}

#[test]
fn directory_trust_accepts_directory_codeterm_created() {
    let (state, tab_id) = state_with_tab(Some("/srv/lanes/a"), Some("/srv/lanes/a/"));
    assert_eq!(
        directory_trust_provenance(state.get_tab(&tab_id).unwrap()),
        Ok(())
    );
}

#[test]
fn directory_trust_refuses_user_supplied_cwd_without_provenance() {
    let (state, tab_id) = state_with_tab(Some("/home/user/project"), None);
    assert_eq!(
        directory_trust_provenance(state.get_tab(&tab_id).unwrap()),
        Err(DirectoryTrustRefusal::NoProvenance)
    );
}

#[test]
fn directory_trust_refuses_cwd_that_differs_from_provenance() {
    let (state, tab_id) = state_with_tab(Some("/home/user/project"), Some("/srv/lanes/a"));
    assert_eq!(
        directory_trust_provenance(state.get_tab(&tab_id).unwrap()),
        Err(DirectoryTrustRefusal::CwdDiffersFromProvenance)
    );
}

#[test]
fn directory_trust_accepts_verbatim_prefixed_cwd_matching_provenance() {
    let (state, tab_id) = state_with_tab(
        Some(r"\\?\C:\Users\User\lane\project"),
        Some(r"C:\Users\User\lane\project"),
    );
    assert_eq!(
        directory_trust_provenance(state.get_tab(&tab_id).unwrap()),
        Ok(())
    );
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn trust_responder_never_writes_on_a_non_trust_dialog() {
    let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../plugins/provider-codex/tests/fixtures/directory-trust/codex-update-available.txt",
    );
    let screen = std::fs::read_to_string(fixture_path).unwrap();
    let (mut mux, tab_id) = agent_tab(Some("/srv/lanes/a"), false);
    mux.get_tab_mut(&tab_id).unwrap().pty_id = Some("pty-update-trust".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("claude"),
    ));

    let decision = fixture
        .service
        .respond_to_directory_trust(&tab_id, Some(&screen));

    assert_eq!(decision, TrustDecision::NoDialog);
    assert!(super::LEDGER
        .lock()
        .tabs
        .get(&tab_id)
        .is_none_or(|trust| trust.answered_frame.is_none()));
    assert_eq!(
        crate::service::output::prompt_step_dispatch_count_for_test("pty-update-trust"),
        0,
        "a non-trust dialog must not dispatch any PTY key"
    );
}

#[test]
fn core_trust_decision_answers_claude_folder_trust_fixtures() {
    let fixture_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../plugins/provider-claude/tests/fixtures/folder-trust");
    let provider = codeterm_plugin::agent_claude::ClaudeProvider::new();
    for (file, step) in [
        ("quick-safety-yes-focused.txt", b"\r".as_slice()),
        ("trust-rule-focus-affirmative.txt", b"\r".as_slice()),
        ("trust-box-focus-refusal.txt", b"j".as_slice()),
        ("trust-numbered-focus-affirmative.txt", b"\r".as_slice()),
    ] {
        let screen = std::fs::read_to_string(fixture_dir.join(file)).expect("Claude trust frame");
        assert_eq!(
            decide(
                true,
                file,
                &screen,
                &parking_lot::Mutex::new(TrustLedger::default()),
                |s| { provider.directory_trust_prompt_response(s) }
            ),
            TrustDecision::Answer {
                step: step.to_vec()
            },
            "provider-classified Claude trust fixture {file}"
        );
    }
}

#[test]
fn directory_trust_refuses_tab_without_cwd() {
    let (state, tab_id) = state_with_tab(None, Some("/srv/lanes/a"));
    assert_eq!(
        directory_trust_provenance(state.get_tab(&tab_id).unwrap()),
        Err(DirectoryTrustRefusal::NoCwd)
    );
}

#[test]
fn directory_trust_provenance_survives_session_restore() {
    let (state, tab_id) = state_with_tab(Some("/srv/lanes/a"), Some("/srv/lanes/a"));
    let restored = MultiplexerState::from_session_snapshot(&state.to_session_snapshot());
    assert_eq!(
        directory_trust_provenance(restored.get_tab(&tab_id).expect("restored tab")),
        Ok(())
    );
}

#[test]
fn spawn_payloads_cannot_supply_provenance() {
    let wire = serde_json::json!({
        "provider": "codex",
        "cwd": "/home/user/project",
        "provisioned_cwd": "/home/user/project",
    });
    let params: crate::service::SpawnAgentParams =
        serde_json::from_value(wire.clone()).expect("spawn params");
    assert_eq!(params.cwd.as_deref(), Some("/home/user/project"));
    assert!(params.provisioned_cwd.is_none());
    let spec: crate::service::spawn::AgentSpawnSpec =
        serde_json::from_value(wire).expect("agent spawn spec");
    assert!(spec.provisioned_cwd.is_none());
}

#[test]
fn spawn_inherits_workspace_provenance_only_in_the_provisioned_directory() {
    let provisioned = Some("C:/data/provisioned-workspaces/bench-1");
    assert_eq!(
        inherited_workspace_provenance(
            Some("C:\\data\\provisioned-workspaces\\bench-1\\"),
            provisioned
        )
        .as_deref(),
        provisioned
    );
    assert!(inherited_workspace_provenance(
        Some("C:/data/provisioned-workspaces/bench-1/user-made"),
        provisioned
    )
    .is_none());
    assert!(inherited_workspace_provenance(Some("C:/Users/user/project"), provisioned).is_none());
    assert!(inherited_workspace_provenance(None, provisioned).is_none());
    assert!(inherited_workspace_provenance(Some("C:/Users/user/project"), None).is_none());
    assert_eq!(
        inherited_workspace_provenance(
            Some(r"\\?\C:\data\provisioned-workspaces\bench-1"),
            Some(r"C:\data\provisioned-workspaces\bench-1"),
        )
        .as_deref(),
        Some(r"C:\data\provisioned-workspaces\bench-1")
    );
}

#[test]
fn user_directory_never_invokes_the_recognizer() {
    let (state, tab_id) = agent_tab(None, false);
    let applies = directory_trust_check_applies(state.get_tab(&tab_id).unwrap());
    assert!(!applies);
    let invoked = Cell::new(false);
    let ledger = parking_lot::Mutex::new(TrustLedger::default());
    let decision = decide(applies, &tab_id, "any screen", &ledger, |_| {
        invoked.set(true);
        answer(b"\r")
    });
    assert_eq!(decision, TrustDecision::NotApplicable);
    assert!(
        !invoked.get(),
        "a user directory must never reach the recognizer"
    );
}

#[test]
fn authenticated_session_is_ignored() {
    let (state, tab_id) = agent_tab(Some("/srv/lanes/a"), true);
    assert!(!directory_trust_check_applies(
        state.get_tab(&tab_id).unwrap()
    ));
}

#[test]
fn tab_without_agent_is_ignored() {
    let (state, tab_id) = state_with_tab(Some("/srv/lanes/a"), Some("/srv/lanes/a"));
    assert!(!directory_trust_check_applies(
        state.get_tab(&tab_id).unwrap()
    ));
}

#[test]
fn provisioned_directory_answers_one_key_per_observed_frame() {
    let (state, tab_id) = agent_tab(Some("/srv/lanes/a"), false);
    let applies = directory_trust_check_applies(state.get_tab(&tab_id).unwrap());
    assert!(applies);
    let ledger = parking_lot::Mutex::new(TrustLedger::default());

    let first = decide(applies, &tab_id, "focus-on-exit", &ledger, |_| answer(b"j"));
    assert_eq!(
        first,
        TrustDecision::Answer {
            step: b"j".to_vec()
        }
    );

    let in_flight = decide(applies, &tab_id, "focus-on-exit", &ledger, |_| answer(b"j"));
    assert_eq!(
        in_flight,
        TrustDecision::Waiting,
        "no second key while one is in flight"
    );

    ledger.lock().settle(&tab_id);
    let same_frame = decide(applies, &tab_id, "focus-on-exit", &ledger, |_| answer(b"j"));
    assert_eq!(
        same_frame,
        TrustDecision::Waiting,
        "an unchanged frame is never answered twice"
    );

    let moved = decide(applies, &tab_id, "focus-on-trust", &ledger, |_| {
        answer(b"\r")
    });
    assert_eq!(
        moved,
        TrustDecision::Answer {
            step: b"\r".to_vec()
        }
    );

    ledger.lock().settle(&tab_id);
    let held = decide(applies, &tab_id, "permissions-variant", &ledger, |_| {
        Some(DirectoryTrustPromptResponse::Hold)
    });
    assert_eq!(held, TrustDecision::Held);
    let gone = decide(applies, &tab_id, "composer", &ledger, |_| None);
    assert_eq!(gone, TrustDecision::NoDialog);
}

#[test]
fn launch_onboarding_unchanged_focus_redraw_does_not_queue_second_key() {
    let capture = include_str!("../../../../plugins/provider-claude/tests/fixtures/onboarding/auto-environment-captured.txt");
    let provider = codeterm_plugin::agent_claude::ClaudeProvider::new();
    let response = provider
        .launch_onboarding_response(capture)
        .expect("captured onboarding prompt");
    assert_eq!(response.step, b"j");
    let ledger = parking_lot::Mutex::new(TrustLedger::default());

    let first = decide_with_frame_identity(
        true,
        "onboarding-redraw",
        capture,
        &response.frame_key,
        &ledger,
        |_| answer(&response.step),
    );
    assert_eq!(first, TrustDecision::Answer { step: b"j".to_vec() });
    ledger.lock().settle("onboarding-redraw");

    let redraw = format!("{capture}\nstatus refreshed");
    let repeated = decide_with_frame_identity(
        true,
        "onboarding-redraw",
        &redraw,
        &response.frame_key,
        &ledger,
        |_| answer(&response.step),
    );
    assert_eq!(
        repeated,
        TrustDecision::Waiting,
        "an extra redraw with unchanged focus must not queue another navigation key"
    );
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn post_launch_idle_planner_answers_onboarding_and_owner_prompt_is_not_auto_answered() {
    use codeterm_types::agent::roles::AgentRole;

    let yes_focused = include_str!("../../../../plugins/provider-claude/tests/fixtures/onboarding/auto-environment-captured.txt");
    let not_now_focused = include_str!("../../../../plugins/provider-claude/tests/fixtures/onboarding/auto-environment-not-now.txt");
    let (mut mux, planner_tab_id) = agent_tab(None, true);
    let planner = mux.get_tab_mut(&planner_tab_id).unwrap();
    planner.role = Some(AgentRole::Manager);
    planner.role_profile = Some("planner".into());
    planner.pty_id = Some("pty-late-onboarding-planner".into());
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let owner_tab_id = mux.create_tab(&workspace_id).expect("owner tab");
    let owner = mux.get_tab_mut(&owner_tab_id).unwrap();
    owner.agent_type = Some("claude".into());
    owner.pty_id = Some("pty-late-onboarding-owner".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        codeterm_plugin::agent_claude::ClaudeProvider::new(),
    ));

    let screen_signal = || {
        std::sync::Arc::new(codeterm_pty::pty::screen_state::ScreenChangeSignal::default())
    };
    assert_eq!(
        fixture.service.respond_to_launch_onboarding(
            &planner_tab_id,
            "claude",
            yes_focused,
            screen_signal(),
        ),
        TrustDecision::Answer { step: b"j".to_vec() },
        "an idle post-launch Planner moves once toward Not now"
    );
    super::LEDGER.lock().settle(&planner_tab_id);
    assert_eq!(
        fixture.service.respond_to_launch_onboarding(
            &planner_tab_id,
            "claude",
            not_now_focused,
            screen_signal(),
        ),
        TrustDecision::Answer { step: b"\r".to_vec() },
        "Enter is submitted only after Not now has focus"
    );
    assert_eq!(
        crate::service::output::prompt_step_dispatch_count_for_test(
            "pty-late-onboarding-planner"
        ),
        2,
        "post-launch handling dispatches exactly j followed by Enter"
    );
    assert_eq!(
        fixture.service.respond_to_launch_onboarding(
            &owner_tab_id,
            "claude",
            yes_focused,
            screen_signal(),
        ),
        TrustDecision::NotApplicable,
        "owner-facing tabs are excluded from the safe auto-response"
    );
    assert_eq!(
        crate::service::output::prompt_step_dispatch_count_for_test(
            "pty-late-onboarding-owner"
        ),
        0
    );

    let prompt = fixture
        .service
        .agent_registry
        .parse_interactive_prompt("claude", yes_focused)
        .expect("owner prompt parse")
        .0;
    let decision = fixture
        .service
        .prompt_detector
        .lock()
        .evaluate_pty_output_parsed(
            Some(owner_tab_id.clone()),
            true,
            true,
            Some(crate::agent::prompt::ResolvedPrompt {
                prompt: prompt.clone(),
                explicit: false,
            }),
        );
    fixture
        .service
        .apply_prompt_decision_with_screen(&decision, Some(yes_focused), false, false);
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&owner_tab_id)
            .and_then(|tab| tab.pending_prompt.as_ref()),
        Some(&prompt),
        "owner-facing tabs keep the prompt available for a human"
    );
}

#[test]
fn trust_dialogs_never_reach_the_legacy_yes_responder() {
    let trust_prompt = PendingPrompt {
        question: "Quick safety check: Is this a project you created or one you trust?".into(),
        options: vec![
            PromptOption {
                label: "Yes, I trust this folder".into(),
                value: "1".into(),
            },
            PromptOption {
                label: "No, exit".into(),
                value: "2".into(),
            },
        ],
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    };
    assert!(
        crate::service::output::legacy_auto_approve_response(&trust_prompt, false).is_some(),
        "precondition: the legacy responder would press Enter on this prompt"
    );
    assert!(
        crate::service::output::legacy_auto_approve_response(&trust_prompt, true).is_none(),
        "a dedicated trust prompt never reaches the legacy responder"
    );

    for owned in [
        TrustDecision::Held,
        TrustDecision::Waiting,
        TrustDecision::Answer {
            step: b"\r".to_vec(),
        },
    ] {
        for (authenticated, structured) in [(false, false), (true, false), (true, true)] {
            assert!(
                !legacy_auto_approve_permitted(&owned, authenticated, structured, false),
                "{owned:?} must keep the legacy responder away"
            );
        }
    }
    for unrecognized in [TrustDecision::NotApplicable, TrustDecision::NoDialog] {
        assert!(
            !legacy_auto_approve_permitted(&unrecognized, false, false, false),
            "a screen-scraped prompt before session authentication is never blindly approved"
        );
    }
    assert!(legacy_auto_approve_permitted(
        &TrustDecision::NotApplicable,
        true,
        false,
        false
    ));
    assert!(legacy_auto_approve_permitted(
        &TrustDecision::NotApplicable,
        false,
        true,
        false
    ));
}

#[test]
fn cwd_provenance_refusal_keeps_trust_prompt_out_of_both_auto_responders() {
    let (state, tab_id) = agent_tab(Some("/srv/lanes/b"), false);
    let tab = state.get_tab(&tab_id).unwrap();
    assert_eq!(
        directory_trust_provenance(tab),
        Err(DirectoryTrustRefusal::CwdDiffersFromProvenance)
    );
    assert!(!directory_trust_check_applies(tab));

    let trust_prompt = PendingPrompt {
        question: "Should this folder's guidance be loaded?".into(),
        options: vec![
            PromptOption {
                label: "Yes, continue".into(),
                value: "1".into(),
            },
            PromptOption {
                label: "No, quit".into(),
                value: "2".into(),
            },
        ],
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    };
    let screen = "Should this folder's guidance be loaded?\n1. Yes, continue\n2. No, quit";

    assert!(!legacy_auto_approve_permitted(
        &TrustDecision::NotApplicable,
        true,
        false,
        true,
    ));
    assert!(crate::service::output::legacy_auto_approve_response(&trust_prompt, true).is_none());
    assert!(
        crate::service::general_agent::blocker::classify(&trust_prompt, screen, true).is_none()
    );
}

#[test]
fn skip_reason_names_why_a_provisioned_agent_is_not_answered() {
    let (state, tab_id) = agent_tab(Some("/srv/lanes/a"), true);
    assert_eq!(
        directory_trust_skip_reason(state.get_tab(&tab_id).unwrap()),
        Some("launch session already authenticated")
    );
    let (state, tab_id) = agent_tab(Some("/srv/lanes/b"), false);
    assert_eq!(
        directory_trust_skip_reason(state.get_tab(&tab_id).unwrap()),
        Some(DirectoryTrustRefusal::CwdDiffersFromProvenance.reason())
    );
    let (state, tab_id) = agent_tab(Some("/srv/lanes/a"), false);
    assert_eq!(
        directory_trust_skip_reason(state.get_tab(&tab_id).unwrap()),
        None,
        "an applicable tab is answered, not skipped"
    );
    let (state, tab_id) = agent_tab(None, false);
    assert_eq!(
        directory_trust_skip_reason(state.get_tab(&tab_id).unwrap()),
        None,
        "a user directory is never logged"
    );
}

#[test]
fn trust_notes_are_logged_once_per_outcome_within_a_budget() {
    let mut ledger = TrustLedger::default();
    assert!(ledger.note("tab", "no trust dialog recognized", 1));
    assert!(
        !ledger.note("tab", "no trust dialog recognized", 1),
        "an unchanged screen is noted once"
    );
    assert!(ledger.note("tab", "no trust dialog recognized", 2));
    for detail in 3..40 {
        ledger.note("tab", "no trust dialog recognized", detail);
    }
    assert!(
        !ledger.note("tab", "provider holds this trust dialog", 99),
        "a tab stops logging once its budget is spent"
    );
    assert!(ledger.note("other", "no trust dialog recognized", 1));
}
