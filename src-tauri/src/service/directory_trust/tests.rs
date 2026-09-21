use std::cell::Cell;

use codeterm_plugin::provider::DirectoryTrustPromptResponse;

use super::{
    decide, directory_trust_check_applies, directory_trust_provenance,
    directory_trust_prompt_candidate, directory_trust_skip_reason, inherited_workspace_provenance,
    legacy_auto_approve_permitted, DirectoryTrustRefusal, TrustDecision, TrustLedger,
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
        Some(r"\\?\C:\Users\dev\lane\project"),
        Some(r"C:\Users\dev\lane\project"),
    );
    assert_eq!(
        directory_trust_provenance(state.get_tab(&tab_id).unwrap()),
        Ok(())
    );
}

#[test]
fn directory_trust_prompt_candidate_uses_active_path_choice_structure() {
    let trust = "> You are in D:\\workspace\\project\n\nShould this folder's guidance be loaded? Project rules can alter how the running agent behaves while working in this folder.\n\n  1. Yes, continue\n> 2. No, quit\n\nPress enter to continue";
    let approval = "Do you want to apply this change? It can modify the selected files in your project.\n\n  1. Yes, apply\n> 2. No, cancel\n\nPress enter to continue";
    let bypass = "> D:\\workspace\\project\n\nShould this folder's local rules be loaded? This enables a bypass for the configured policies and removes the usual restrictions.\n\n  1. Yes, continue\n> 2. No, quit\n\nPress enter to continue";
    let model_picker = "> You are in D:\\workspace\\project\n\nSelect a model\n\n  1. Model A\n> 2. Model B\n  3. Model C\n\nPress enter to select";
    let answer_list = "> You are in D:\\workspace\\project\n\nHere are the choices we discussed:\n  1. First answer\n  2. Second answer\n  3. Third answer";
    let one_paragraph_approval = "> D:\\workspace\\project\n\nWould you like to apply this change? It updates the selected files and records the result in the project history.\n\n  1. Yes, apply\n> 2. No, cancel\n\nPress enter to continue";
    let unselected_trust = trust.replace("> 2. No", " 2. No");

    assert!(directory_trust_prompt_candidate(trust));
    assert!(!directory_trust_prompt_candidate(approval));
    assert!(!directory_trust_prompt_candidate(bypass));
    assert!(!directory_trust_prompt_candidate(model_picker));
    assert!(!directory_trust_prompt_candidate(answer_list));
    assert!(!directory_trust_prompt_candidate(one_paragraph_approval));
    assert!(!directory_trust_prompt_candidate(&unselected_trust));
}

#[test]
fn directory_trust_prompt_candidate_matches_real_fixture_frames() {
    let fixture_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../plugins/provider-codex/tests/fixtures/directory-trust");
    let cases: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(fixture_dir.join("cases.json")).unwrap(),
    )
    .unwrap();

    for case in cases.as_array().unwrap() {
        let file = case["file"].as_str().unwrap();
        let expected = case["expect"].as_str().unwrap() != "none";
        let screen = std::fs::read_to_string(fixture_dir.join(file)).unwrap();
        assert_eq!(
            directory_trust_prompt_candidate(&screen),
            expected,
            "{file}"
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
