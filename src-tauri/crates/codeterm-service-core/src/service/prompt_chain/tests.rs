use super::*;
use crate::multiplexer::types::TabAudience;
use crate::service::test_fixture::ServiceFixture;
use codeterm_notification_core::contract::{NotificationAction, NotifyLevel};
use codeterm_types::pty::{AgentRole, BootStage, PromptIdentity, PromptOption};

fn key(id: &str) -> ChainKey {
    ChainKey {
        tab_id: id.into(),
        launch_id: format!("launch-{id}"),
    }
}

#[test]
fn trust_then_login_keeps_one_answer_and_one_kind_notice() {
    let mut book = PromptChainBook::default();
    let key = key("trust-login");
    assert_eq!(book.admit(&key, "trust".into(), None), Admission::Allowed);
    assert!(book.is_active(&key));
    assert!(book.blocks_keys(&key));
    book.finish(&key);
    assert_eq!(book.entries[&key].answers, 1);
    assert!(book.claim_notice(&key, "login"));
    assert!(!book.claim_notice(&key, "login"));
    assert!(book.blocks_keys(&key));
}

#[test]
fn trust_onboarding_ready_counts_two_answers_and_stops_observing() {
    let mut book = PromptChainBook::default();
    let key = key("trust-onboarding-ready");
    assert_eq!(book.admit(&key, "trust".into(), None), Admission::Allowed);
    assert_eq!(
        book.admit(&key, "onboarding".into(), None),
        Admission::Allowed
    );
    assert_eq!(book.entries[&key].answers, 2);
    book.finish(&key);
    assert!(!book.is_active(&key));
}

#[test]
fn repeated_prompt_is_not_answered_again_and_five_is_the_limit() {
    let mut repeated = PromptChainBook::default();
    let repeated_key = key("repeat");
    assert_eq!(
        repeated.admit(&repeated_key, "same".into(), None),
        Admission::Allowed
    );
    assert_eq!(
        repeated.admit(&repeated_key, "same".into(), None),
        Admission::Repeated
    );
    assert_eq!(repeated.entries[&repeated_key].answers, 1);

    let mut bounded = PromptChainBook::default();
    let bounded_key = key("bounded");
    for index in 0..MAX_AUTOMATIC_ANSWERS {
        assert_eq!(
            bounded.admit(&bounded_key, format!("prompt-{index}"), None),
            Admission::Allowed
        );
    }
    assert_eq!(
        bounded.admit(
            &bounded_key,
            format!("prompt-{}", MAX_AUTOMATIC_ANSWERS - 1),
            Some("enter".into())
        ),
        Admission::Allowed
    );
    assert_eq!(bounded.entries[&bounded_key].answers, MAX_AUTOMATIC_ANSWERS);
    assert_eq!(
        bounded.admit(&bounded_key, "sixth".into(), None),
        Admission::Limit
    );
    assert_eq!(bounded.entries[&bounded_key].answers, MAX_AUTOMATIC_ANSWERS);
}

#[test]
fn cron_blocker_notice_uses_the_cron_name() {
    let subject = notice_subject("Bizet", Some("Nightly QA"));
    let body = notice_body(&subject, "login");
    assert!(body.contains("Nightly QA"));
    assert!(body.contains("login"));
    assert!(!body.contains("Bizet"));
}

#[test]
fn move_then_enter_is_one_answer_and_completed_repeat_stays_blocked() {
    let mut book = PromptChainBook::default();
    let key = key("trust-steps");
    assert_eq!(
        book.admit(&key, "trust-prompt".into(), Some("down".into())),
        Admission::Allowed
    );
    assert_eq!(
        book.admit(&key, "trust-prompt".into(), Some("enter".into())),
        Admission::Allowed
    );
    assert_eq!(book.entries[&key].answers, 1);
    book.finish(&key);
    assert_eq!(
        book.admit(&key, "trust-prompt".into(), Some("enter".into())),
        Admission::Repeated
    );
    assert_eq!(book.entries[&key].answers, 1);
}

#[test]
fn unsupported_prompt_shapes_have_owner_blocker_kinds() {
    let mut prompt = PendingPrompt {
        request_id: None,
        question: "opaque-question".into(),
        options: Vec::new(),
        sections: None,
        multi_select: Some(false),
        submit_available: None,
        selected_values: None,
    };
    assert_eq!(blocker_kind(&prompt), "free_text");
    prompt.options = (0..11)
        .map(|index| codeterm_types::pty::PromptOption {
            label: format!("option-{index}"),
            value: format!("value-{index}"),
        })
        .collect();
    assert_eq!(blocker_kind(&prompt), "too_many_options");
    prompt.multi_select = Some(true);
    assert_eq!(blocker_kind(&prompt), "multi_select");
}

#[test]
fn owner_prompt_notice_is_once_per_blocker_and_launch() {
    let mut book = PromptChainBook::default();
    let launch = key("notice-launch");
    let repeated = PendingPrompt {
        request_id: None,
        question: "opaque-question".into(),
        options: Vec::new(),
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    };
    let multi_select = PendingPrompt {
        question: "another-question".into(),
        multi_select: Some(true),
        ..repeated.clone()
    };
    let free_text_kind = prompt_notice_kind(&repeated);
    let multi_select_kind = prompt_notice_kind(&multi_select);
    let updated_multi_select = PendingPrompt {
        options: vec![codeterm_types::pty::PromptOption {
            label: "updated-option".into(),
            value: "updated-value".into(),
        }],
        ..multi_select.clone()
    };

    assert!(book.claim_notice(&launch, &free_text_kind));
    assert!(!book.claim_notice(&launch, &free_text_kind));
    assert!(book.claim_notice(&launch, &multi_select_kind));
    assert_eq!(prompt_notice_kind(&updated_multi_select), multi_select_kind);
    assert!(!book.claim_notice(&launch, &prompt_notice_kind(&updated_multi_select)));
    let parent_route = prompt_parent_route_kind(&multi_select);
    assert!(book.claim_notice(&launch, &parent_route));
    assert!(!book.claim_notice(&launch, &prompt_parent_route_kind(&updated_multi_select)));
    book.clear_parent_routes(&launch);
    assert!(book.claim_notice(&launch, &parent_route));
    assert!(book.claim_notice(&key("next-launch"), &free_text_kind));
}

#[test]
fn template_launch_chain_then_pending_route_stores_one_owner_question() {
    let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let manager = mux.create_tab(&workspace).unwrap();
    mux.set_orchestrator_role(&manager).unwrap();
    let team = mux
        .resolve_or_mint_team_for_parent(&manager)
        .unwrap()
        .team_id;
    let params = crate::service::orchestrator::SpawnAgentParams {
        agent: Some("opaque-harness".into()),
        role: Some("Orchestrator".into()),
        can_spawn: Some(true),
        workspace_id: Some(workspace),
        user_created: true,
        existing_tab_id: Some(manager.clone()),
        skip_orchestrator_onboarding: Some(true),
        ..Default::default()
    };
    crate::service::orchestrator::initialize_ordinary_pty_agent_tab(
        mux.get_tab_mut(&manager).unwrap(),
        &params,
    );
    mux.set_orchestrator_role(&manager).unwrap();
    let tab = mux.get_tab(&manager).unwrap();
    assert_eq!(tab.audience, TabAudience::User);
    assert_eq!(tab.role, Some(AgentRole::Manager));
    assert_eq!(tab.role_profile.as_deref(), Some("orchestrator"));
    assert_eq!(tab.team_id.as_deref(), Some(team.as_str()));
    assert!(tab.agent_booting);

    let fixture = ServiceFixture::new(mux);
    fixture.service.set_and_lock_tab_agent_type_from(
        &manager,
        params.agent.clone().unwrap(),
        crate::session::lifecycle::AgentTypeWriteOrigin::Spawn,
        None,
    );
    assert!(crate::service::unattended_launch::at_owned_launch_prompt(
        fixture.service.mux.read().get_tab(&manager).unwrap()
    ));
    fixture
        .service
        .emit_boot_stage(&manager, BootStage::LaunchWritten, None);
    let prompt = PendingPrompt {
        request_id: None,
        question: "Opaque external-file question".into(),
        options: vec![
            PromptOption {
                label: "Opaque decline".into(),
                value: "decline-value".into(),
            },
            PromptOption {
                label: "Opaque allow".into(),
                value: "allow-value".into(),
            },
        ],
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    };
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&manager).unwrap();
        tab.agent_booting = false;
        assert!(!tab.launch_session_authenticated());
        tab.pending_prompt = Some(prompt.clone());
        tab.pending_prompt_kind = Some(PromptKind::Provider);
    }
    let chain = fixture.service.prompt_chain_key(&manager).unwrap();
    assert_eq!(
        CHAINS
            .lock()
            .admit(&chain, "opaque-launch-gate".into(), None),
        Admission::Allowed
    );
    assert!(fixture
        .service
        .handle_prompt_chain_blocker(&manager, &prompt));
    assert!(!CHAINS.lock().entries[&chain]
        .notices
        .contains(&prompt_notice_kind(&prompt)));
    fixture
        .service
        .route_pending_prompt(&manager, &prompt, PromptKind::Provider);
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&manager)
            .unwrap()
            .boot_stage,
        Some(BootStage::LaunchWritten)
    );
    let first_notice = {
        let store = fixture.service.notif.lock();
        let records = store.list(false);
        let notices = records
            .iter()
            .filter(|notice| notice.tab_id.as_deref() == Some(manager.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(notices.len(), 1, "the first route must store the notification before the readiness observer changes the stage");
        assert_eq!(notices[0].level, Some(NotifyLevel::ActionNeeded));
        notices[0].id.clone()
    };
    fixture
        .service
        .emit_boot_stage(&manager, BootStage::AwaitingOwnerPrompt, None);
    fixture
        .service
        .route_pending_prompt(&manager, &prompt, PromptKind::Provider);
    let identity = PromptIdentity::new(PromptKind::Provider, &prompt);
    let notification_id = {
        let store = fixture.service.notif.lock();
        let records = store.list(false);
        let notices = records
            .iter()
            .filter(|notice| notice.tab_id.as_deref() == Some(manager.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].id, first_notice);
        assert_eq!(notices[0].level, Some(NotifyLevel::ActionNeeded));
        assert_eq!(
            notices[0].actions,
            Some(
                prompt
                    .options
                    .iter()
                    .map(|option| NotificationAction::RespondPromptOption {
                        label: option.label.clone(),
                        value: option.value.clone(),
                        identity: identity.clone(),
                    })
                    .collect()
            )
        );
        notices[0].id.clone()
    };
    fixture
        .service
        .answer_prompt_notifications(&manager, &identity, 1, "test-owner");
    {
        let store = fixture.service.notif.lock();
        let records = store.list(false);
        let answered = records
            .iter()
            .find(|notice| notice.id == notification_id)
            .unwrap();
        assert!(answered.resolved);
    }
    fixture.service.apply_prompt_decision_with_kind(
        &crate::agent::prompt::PromptDecision::PromptCleared {
            tab_id: manager.clone(),
        },
        None,
        PromptKind::Provider,
        true,
    );
    fixture.service.apply_prompt_decision_with_kind(
        &crate::agent::prompt::PromptDecision::NewPrompt {
            tab_id: manager.clone(),
            prompt: prompt.clone(),
        },
        None,
        PromptKind::Provider,
        true,
    );
    assert_eq!(
        fixture
            .service
            .notif
            .lock()
            .list(false)
            .iter()
            .filter(|notice| notice.tab_id.as_deref() == Some(manager.as_str()) && !notice.resolved)
            .count(),
        1
    );
    fixture.service.close_tab(&manager).unwrap();
    assert!(fixture
        .service
        .notif
        .lock()
        .list(false)
        .iter()
        .filter(|notice| notice.tab_id.as_deref() == Some(manager.as_str()))
        .all(|notice| notice.resolved));
}
