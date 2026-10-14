use codeterm_plugin::provider::{AgentProvider, DirectoryTrustPromptResponse};

use super::{
    decide, decide_with_frame_identity, directory_trust_check_applies, directory_trust_occurrence,
    directory_trust_provenance, directory_trust_skip_reason, inherited_workspace_provenance,
    legacy_auto_approve_permitted, shared_directory_trust_check_applies, DirectoryTrustRefusal,
    SharedTrustCache, TrustDecision, TrustLedger, LEDGER, SHARED_TRUST,
};
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{PendingPrompt, PromptOption};

fn trust_prompt() -> PendingPrompt {
    PendingPrompt {
        request_id: None,
        question: "directory trust question".into(),
        options: vec![
            PromptOption {
                label: "Trust once".into(),
                value: "1".into(),
            },
            PromptOption {
                label: "Trust always".into(),
                value: "2".into(),
            },
        ],
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    }
}

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

#[test]
fn supporter_first_opinion_model_onboarding_and_deterministic_responder_share_one_frame() {
    let tab = uuid::Uuid::new_v4().to_string();
    let frame = "opaque-focus-frame";
    assert!(super::claim_model_onboarding_step(&tab, "launch:1", frame));
    assert_eq!(
        super::observation_revision(&tab),
        super::model_onboarding_step_revision(frame)
    );
    super::finish_model_onboarding_step(&tab, "launch:1", frame, true);
    let frame_identity = format!("launch-onboarding:{frame}");
    assert_eq!(
        decide_with_frame_identity(
            true,
            &tab,
            "opaque-screen",
            &frame_identity,
            &super::LEDGER,
            |_| answer(b"\r")
        ),
        TrustDecision::Waiting
    );
    assert!(super::claim_model_onboarding_step(&tab, "launch:2", frame));
    super::finish_model_onboarding_step(&tab, "launch:2", frame, true);
}

#[test]
fn supporter_first_opinion_model_onboarding_stale_cleanup_does_not_release_new_owner() {
    let tab = uuid::Uuid::new_v4().to_string();
    assert!(super::claim_model_onboarding_step(
        &tab, "launch:1", "frame-a"
    ));
    super::finish_model_onboarding_step(&tab, "launch:1", "frame-a", false);
    assert!(super::claim_model_onboarding_step(
        &tab, "launch:1", "frame-a"
    ));
    super::finish_model_onboarding_step(&tab, "launch:1", "frame-a", true);
    assert!(super::claim_model_onboarding_step(
        &tab, "launch:2", "frame-a"
    ));
    super::finish_model_onboarding_step(&tab, "launch:1", "frame-a", false);
    assert!(!super::claim_model_onboarding_step(
        &tab, "launch:2", "frame-a"
    ));
    super::finish_model_onboarding_step(&tab, "launch:2", "frame-a", true);
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
fn trust_responder_never_writes_on_a_non_trust_dialog() {
    let fixture_path = std::path::PathBuf::from(crate::test_support::fixture_manifest_dir()).join(
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
    let fixture_dir = std::path::PathBuf::from(crate::test_support::fixture_manifest_dir())
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
fn user_directory_never_invokes_the_provisioned_recognizer() {
    let (state, tab_id) = agent_tab(None, false);
    let tab = state.get_tab(&tab_id).unwrap();
    assert!(!directory_trust_check_applies(tab));
    assert!(shared_directory_trust_check_applies(tab));
}

#[test]
fn fresh_team_role_tab_uses_the_agent_owned_trust_path() {
    let (mut state, tab_id) = agent_tab(None, false);
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.role = Some(codeterm_types::agent::roles::AgentRole::Worker);
    tab.team_id = Some("team-fresh-repo".into());

    assert!(directory_trust_check_applies(tab));
    assert!(!shared_directory_trust_check_applies(tab));
}

#[test]
fn mismatched_provisioned_cwd_never_uses_shared_trust() {
    let (state, tab_id) = agent_tab(Some("/srv/lanes/original"), false);
    let tab = state.get_tab(&tab_id).unwrap();
    assert!(!directory_trust_check_applies(tab));
    assert!(!shared_directory_trust_check_applies(tab));
}

#[test]
fn shared_trust_cache_is_exact_path_scoped_and_invalidated_on_carry() {
    let mut cache = SharedTrustCache::default();
    cache.trusted.insert(
        ("provider-a".into(), r"C:\Users\owner\Trusted".into()),
        true,
    );
    cache
        .checking
        .insert(("provider-a".into(), r"C:\Users\owner\Pending".into()), 0);
    assert_eq!(
        cache
            .trusted
            .get(&("provider-a".into(), r"C:\Users\owner\Trusted".into())),
        Some(&true)
    );
    assert_eq!(
        cache
            .trusted
            .get(&("provider-b".into(), r"C:\Users\owner\Trusted".into())),
        None
    );
    cache.invalidate();
    assert!(cache.trusted.is_empty());
    assert!(cache.checking.is_empty());
    assert_eq!(cache.generation, 1);
}

#[test]
fn trusted_sibling_prompt_uses_the_bound_directory_trust_responder() {
    let (mut mux, tab_id) = agent_tab(None, false);
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("codex".into());
    tab.cwd = Some(r"C:\Users\owner\Trusted".into());
    tab.pty_id = Some("pty-shared-trust".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_directory_trust_response(
            "codex",
            DirectoryTrustPromptResponse::Answer {
                step: b"\r".to_vec(),
            },
        )
        .with_directory_trust_prompt(trust_prompt())
        .with_directory_trusted_path(r"C:\Users\owner\Trusted"),
    ));

    super::invalidate_shared_trust_cache();
    SHARED_TRUST
        .lock()
        .trusted
        .insert(("codex".into(), r"C:\Users\owner\Trusted".into()), true);
    fixture
        .service
        .respond_to_shared_trust_prompt(&tab_id, r"C:\Users\owner\Trusted");
    assert_eq!(
        crate::service::output::prompt_step_dispatch_count_for_test("pty-shared-trust"),
        1,
        "an already-open sibling prompt receives one bound trust key",
    );
}

#[test]
fn untrusted_shared_lookup_stores_one_owner_prompt_and_notifies_once() {
    let folder = tempfile::tempdir().expect("owner checkout");
    let cwd = folder.path().to_string_lossy().into_owned();
    let (mut mux, tab_id) = state_with_tab(Some(&cwd), None);
    let tab = mux.get_tab_mut(&tab_id).expect("tab");
    tab.agent_type = Some("fixture".into());
    tab.pty_id = Some("pty-owner-trust".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let prompt = PendingPrompt {
        request_id: None,
        question: "owner-trust-question-1".into(),
        options: vec![
            PromptOption {
                label: "owner-trust-yes".into(),
                value: "1".into(),
            },
            PromptOption {
                label: "owner-trust-no".into(),
                value: "2".into(),
            },
        ],
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    };
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_directory_trust_response(
            "fixture",
            DirectoryTrustPromptResponse::Answer {
                step: b"\r".to_vec(),
            },
        )
        .with_directory_trust_prompt(prompt.clone()),
    ));
    let key = ("fixture".to_string(), cwd.clone());
    {
        let mut cache = SHARED_TRUST.lock();
        let generation = cache.generation;
        cache.checking.insert(key.clone(), generation);
    }

    assert_eq!(
        fixture
            .service
            .respond_to_directory_trust(&tab_id, Some("owner trust screen")),
        TrustDecision::NotApplicable,
        "an in-flight lookup leaves the prompt to the owner instead of claiming it"
    );

    {
        let mut cache = SHARED_TRUST.lock();
        cache.checking.remove(&key);
        cache.trusted.insert(key, false);
    }
    fixture
        .service
        .surface_shared_directory_trust_prompts(&cwd, "fixture");
    fixture
        .service
        .surface_shared_directory_trust_prompts(&cwd, "fixture");

    assert!(crate::service::launch_readiness::owner_prompt_is_active(
        &tab_id
    ));
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .and_then(|tab| tab.pending_prompt.as_ref()),
        Some(&prompt)
    );
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .and_then(|tab| tab.pending_prompt_kind),
        Some(codeterm_types::pty::PromptKind::DirectoryTrust)
    );
    {
        let notification_state = fixture.service.notif.lock();
        let notices = notification_state.list(false);
        assert!(notices.is_empty());
    }
    assert_eq!(
        crate::service::output::prompt_step_dispatch_count_for_test("pty-owner-trust"),
        0,
        "an untrusted folder must never receive an affirmative trust key"
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
fn directory_trust_occurrence_is_claimed_once_until_prompt_clear() {
    let (state, tab_id) = agent_tab(Some("/srv/lanes/a"), false);
    let applies = directory_trust_check_applies(state.get_tab(&tab_id).unwrap());
    assert!(applies);
    let occurrence = directory_trust_occurrence(&trust_prompt());
    let occurrence_id = super::frame_id(&occurrence);
    let nonce = "launch-trust-once";
    LEDGER
        .lock()
        .observe_trust_prompt(&tab_id, Some(occurrence_id), true, nonce);

    let first = super::decide_with_occurrence(
        applies,
        &tab_id,
        "focus-on-exit",
        &occurrence,
        &LEDGER,
        |_| answer(b"j"),
    );
    assert_eq!(
        first,
        TrustDecision::Answer {
            step: b"j".to_vec()
        }
    );

    let redrawn = super::decide_with_occurrence(
        applies,
        &tab_id,
        "focus-on-trust",
        &occurrence,
        &LEDGER,
        |_| answer(b"j"),
    );
    assert_eq!(
        redrawn,
        TrustDecision::Waiting,
        "a screen redraw does not create another prompt occurrence"
    );

    LEDGER
        .lock()
        .observe_trust_prompt(&tab_id, None, false, nonce);
    assert!(super::trust_prompt_resolved_for_launch(&tab_id, nonce));
    LEDGER
        .lock()
        .observe_trust_prompt(&tab_id, Some(occurrence_id), true, nonce);
    assert_eq!(
        super::decide_with_occurrence(
            applies,
            &tab_id,
            "same prompt after a classified clear",
            &occurrence,
            &LEDGER,
            |_| answer(b"j"),
        ),
        TrustDecision::Answer {
            step: b"j".to_vec()
        },
        "a reappearing prompt after a visible clear is a new occurrence"
    );
    LEDGER.lock().forget(&tab_id);
}

#[test]
fn deterministic_and_model_trust_responders_racing_claim_once() {
    let tab_id = uuid::Uuid::new_v4().to_string();
    let occurrence = directory_trust_occurrence(&trust_prompt());
    let occurrence_id = super::frame_id(&occurrence);
    let nonce = "launch-trust-race";
    LEDGER
        .lock()
        .observe_trust_prompt(&tab_id, Some(occurrence_id), true, nonce);

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let deterministic_barrier = barrier.clone();
    let deterministic_occurrence = occurrence.clone();
    let deterministic_tab = tab_id.clone();
    let deterministic = std::thread::spawn(move || {
        deterministic_barrier.wait();
        super::decide_with_occurrence(
            true,
            &deterministic_tab,
            "redrawn trust screen",
            &deterministic_occurrence,
            &LEDGER,
            |_| answer(b"j"),
        )
    });
    let model_barrier = barrier.clone();
    let model_occurrence = occurrence.clone();
    let model_tab = tab_id.clone();
    let model = std::thread::spawn(move || {
        model_barrier.wait();
        super::claim_model_trust_step(&model_tab, &model_occurrence)
    });

    barrier.wait();
    let decision = deterministic.join().expect("deterministic responder");
    let model_claimed = model.join().expect("model responder");
    assert_eq!(
        matches!(decision, TrustDecision::Answer { .. }) as usize + model_claimed as usize,
        1,
        "only one responder owns the provider-declared trust transaction"
    );
    if model_claimed {
        super::finish_model_trust_step(&tab_id, &occurrence, true);
    }
    assert_eq!(
        super::decide_with_occurrence(
            true,
            &tab_id,
            "another redraw",
            &occurrence,
            &LEDGER,
            |_| answer(b"j"),
        ),
        TrustDecision::Waiting
    );
    LEDGER
        .lock()
        .observe_trust_prompt(&tab_id, None, false, nonce);
    LEDGER.lock().forget(&tab_id);
}

#[test]
fn fresh_team_directory_trust_answers_one_transaction_under_race() {
    let (mut mux, tab_id) = state_with_tab(Some("/repo/fresh-team"), None);
    let tab = mux.get_tab_mut(&tab_id).expect("team tab");
    tab.agent_type = Some("claude".into());
    tab.role = Some(codeterm_types::agent::roles::AgentRole::Worker);
    tab.team_id = Some("team-fresh-repo".into());
    tab.pty_id = Some("pty-fresh-team-trust".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_directory_trust_response(
            "claude",
            DirectoryTrustPromptResponse::Answer {
                step: b"j".to_vec(),
            },
        )
        .with_directory_trust_prompt(trust_prompt())
        .with_live_prompt_response(1, vec![b"j".to_vec(), b"\r".to_vec()]),
    ));

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let first_service = fixture.service.clone();
    let first_barrier = barrier.clone();
    let first_tab = tab_id.clone();
    let first = std::thread::spawn(move || {
        first_barrier.wait();
        first_service.respond_to_directory_trust(&first_tab, Some("folder trust"))
    });
    let second_service = fixture.service.clone();
    let second_barrier = barrier.clone();
    let second_tab = tab_id.clone();
    let second = std::thread::spawn(move || {
        second_barrier.wait();
        second_service.respond_to_directory_trust(&second_tab, Some("folder trust"))
    });

    barrier.wait();
    let decisions = [
        first.join().expect("first trust responder"),
        second.join().expect("second trust responder"),
    ];
    assert_eq!(
        decisions
            .iter()
            .filter(|decision| {
                matches!(decision, &TrustDecision::Answer { step } if step.as_slice() == b"j")
            })
            .count(),
        1
    );
    assert_eq!(
        decisions
            .iter()
            .filter(|decision| matches!(decision, &&TrustDecision::Waiting))
            .count(),
        1
    );
    assert_eq!(
        crate::service::output::prompt_step_dispatch_count_for_test("pty-fresh-team-trust"),
        1,
        "the Team tab receives one folder trust transaction"
    );
}

#[test]
fn observed_trust_clear_resolves_only_the_current_launch_binding() {
    let trust_screen = include_str!(
        "../../../../plugins/provider-claude/tests/fixtures/folder-trust/quick-safety-no-first.txt"
    );
    let (mut mux, tab_id) = agent_tab(None, false);
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.role = Some(codeterm_types::agent::roles::AgentRole::Worker);
    tab.team_id = Some("team-fresh-repo".into());
    tab.session_binding_nonce = Some("binding-trust-clear".into());
    tab.pty_id = Some("pty-trust-clear-binding".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        codeterm_plugin::agent_claude::ClaudeProvider::new(),
    ));

    super::observe_directory_trust_screen(&fixture.service, &tab_id, "claude", trust_screen);
    assert!(!super::trust_prompt_resolved_for_launch(
        &tab_id,
        "binding-trust-clear"
    ));
    super::observe_directory_trust_screen(&fixture.service, &tab_id, "claude", "composer ready");
    assert!(super::trust_prompt_resolved_for_launch(
        &tab_id,
        "binding-trust-clear"
    ));
    assert!(!super::trust_prompt_resolved_for_launch(
        &tab_id,
        "a-later-launch"
    ));
}

#[test]
fn refused_team_folder_trust_prompt_notifies_owner_once_with_tab_identity() {
    let (mut mux, tab_id) = state_with_tab(Some("/repo/fresh-team"), None);
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("claude".into());
    tab.role = Some(codeterm_types::agent::roles::AgentRole::Worker);
    tab.team_id = Some("team-fresh-repo".into());
    tab.title = "Fresh Team Worker".into();
    tab.audience = crate::multiplexer::types::TabAudience::User;
    tab.pty_id = Some("pty-held-team-trust".into());
    let prompt = trust_prompt();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_directory_trust_response(
            "claude",
            DirectoryTrustPromptResponse::Hold,
        )
        .with_directory_trust_prompt(prompt.clone()),
    ));

    fixture.service.apply_prompt_decision_with_kind(
        &crate::agent::prompt::PromptDecision::NewPrompt {
            tab_id: tab_id.clone(),
            prompt: prompt.clone(),
        },
        Some("folder trust"),
        codeterm_types::pty::PromptKind::DirectoryTrust,
        false,
    );

    {
        let notification_state = fixture.service.notif.lock();
        let notices = notification_state.list(false);
        assert!(notices.is_empty());
    }

    fixture.service.route_pending_prompt(
        &tab_id,
        &prompt,
        codeterm_types::pty::PromptKind::DirectoryTrust,
    );
    assert!(fixture.service.notif.lock().list(false).is_empty());
    let tab = fixture.service.mux.read().get_tab(&tab_id).unwrap().clone();
    assert_eq!(tab.pending_prompt, Some(prompt));
    assert_eq!(
        tab.pending_prompt_kind,
        Some(codeterm_types::pty::PromptKind::DirectoryTrust)
    );
    assert_eq!(tab.team_id.as_deref(), Some("team-fresh-repo"));
}

#[test]
fn provisioned_directory_trust_still_uses_provenance_without_shared_trust() {
    let (mut mux, tab_id) = agent_tab(Some("/srv/lanes/a"), false);
    mux.get_tab_mut(&tab_id).unwrap().pty_id = Some("pty-provisioned-trust".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_directory_trust_response(
            "claude",
            DirectoryTrustPromptResponse::Answer {
                step: b"\r".to_vec(),
            },
        )
        .with_directory_trust_prompt(trust_prompt()),
    ));
    let decision = fixture
        .service
        .respond_to_directory_trust(&tab_id, Some("recognized trust prompt"));
    assert_eq!(
        decision,
        TrustDecision::Answer {
            step: b"\r".to_vec()
        }
    );
    assert_eq!(
        crate::service::output::prompt_step_dispatch_count_for_test("pty-provisioned-trust"),
        1
    );
}

#[test]
fn trust_trusted_dir_by_provenance_auto_answers_no_notification() {
    let (mut mux, tab_id) = agent_tab(Some("/srv/lanes/a"), false);
    mux.get_tab_mut(&tab_id).unwrap().pty_id = Some("pty-untrusted-repo".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_directory_trust_response(
            "claude",
            DirectoryTrustPromptResponse::Answer {
                step: b"\r".to_vec(),
            },
        )
        .with_directory_trust_prompt(trust_prompt())
        .with_directory_trust_state(codeterm_plugin::provider::DirectoryTrustState::Untrusted),
    ));
    let decision = fixture
        .service
        .respond_to_directory_trust(&tab_id, Some("recognized trust prompt"));
    assert_eq!(
        decision,
        TrustDecision::Answer {
            step: b"\r".to_vec()
        }
    );
    assert_eq!(
        crate::service::output::prompt_step_dispatch_count_for_test("pty-untrusted-repo"),
        1
    );
    assert!(fixture
        .service
        .notif
        .lock()
        .list(false)
        .iter()
        .all(|notice| notice.level
            != Some(codeterm_notification_core::contract::NotifyLevel::ActionNeeded)));
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
    assert_eq!(
        first,
        TrustDecision::Answer {
            step: b"j".to_vec()
        }
    );
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

    let screen_signal =
        || std::sync::Arc::new(codeterm_pty::pty::screen_state::ScreenChangeSignal::default());
    assert_eq!(
        fixture.service.respond_to_launch_onboarding(
            &planner_tab_id,
            "claude",
            yes_focused,
            screen_signal(),
        ),
        TrustDecision::Answer {
            step: b"j".to_vec()
        },
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
        TrustDecision::Answer {
            step: b"\r".to_vec()
        },
        "Enter is submitted only after Not now has focus"
    );
    assert_eq!(
        crate::service::output::prompt_step_dispatch_count_for_test("pty-late-onboarding-planner"),
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
        crate::service::output::prompt_step_dispatch_count_for_test("pty-late-onboarding-owner"),
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
            &owner_tab_id,
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
        request_id: None,
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
        request_id: None,
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

#[test]
fn unattended_launch_prompt_model_trust_step_shares_the_deterministic_frame() {
    let tab = uuid::Uuid::new_v4().to_string();
    let occurrence = directory_trust_occurrence(&trust_prompt());
    let occurrence_id = super::frame_id(&occurrence);
    let nonce = "model-trust-launch";
    LEDGER
        .lock()
        .observe_trust_prompt(&tab, Some(occurrence_id), true, nonce);
    assert!(super::claim_model_trust_step(&tab, &occurrence));
    assert_eq!(
        super::observation_revision(&tab),
        super::model_trust_step_revision(&occurrence)
    );
    assert_eq!(
        super::decide_with_occurrence(
            true,
            &tab,
            "focus on first option",
            &occurrence,
            &LEDGER,
            |_| answer(b"\r"),
        ),
        TrustDecision::Waiting,
        "the deterministic responder never types while the model step is in flight"
    );
    super::finish_model_trust_step(&tab, &occurrence, true);
    assert_eq!(
        super::decide_with_occurrence(true, &tab, "redrawn focus", &occurrence, &LEDGER, |_| {
            answer(b"\r")
        },),
        TrustDecision::Waiting,
        "an answered prompt occurrence is never answered twice"
    );
    assert!(!super::claim_model_trust_step(&tab, &occurrence));
    LEDGER.lock().observe_trust_prompt(&tab, None, false, nonce);
    LEDGER
        .lock()
        .observe_trust_prompt(&tab, Some(occurrence_id), true, nonce);
    assert!(super::claim_model_trust_step(&tab, &occurrence));
    super::finish_model_trust_step(&tab, &occurrence, false);
    assert!(super::claim_model_trust_step(&tab, &occurrence));
    super::finish_model_trust_step(&tab, &occurrence, true);
    super::LEDGER.lock().forget(&tab);
}
