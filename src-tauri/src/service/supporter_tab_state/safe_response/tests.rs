use super::*;
use codeterm_types::pty::PromptOption;
use std::time::{Duration, Instant};

fn target() -> Target {
    Target {
        kind: Kind::Update,
        prompt: codeterm_types::pty::PendingPrompt {
            question: "opaque-question".into(),
            options: vec![
                PromptOption {
                    label: "opaque-a".into(),
                    value: "install".into(),
                },
                PromptOption {
                    label: "opaque-b".into(),
                    value: "skip-token".into(),
                },
            ],
            sections: None,
            multi_select: Some(false),
            submit_available: Some(false),
            selected_values: None,
        },
        choice: "skip-token".into(),
        frame_key: "opaque-frame".into(),
        step: b"\r".to_vec(),
    }
}

fn evidence() -> Evidence<'static> {
    Evidence {
        observations: 2,
        confidence: 0.95,
        qualified: true,
        current: true,
        eligible: true,
        model_choice: Some(("skip-token", 0.95)),
    }
}

#[test]
fn supporter_first_opinion_safe_response_never_authorizes_without_evidence() {
    let target = target();
    assert!(authorized(&target, &evidence()));
    for arm in 0..7 {
        let mut e = evidence();
        match arm {
            0 => e.observations = 1,
            1 => e.confidence = 0.89,
            2 => e.qualified = false,
            3 => e.current = false,
            4 => e.eligible = false,
            5 => e.model_choice = Some(("install", 0.99)),
            _ => e.model_choice = Some(("skip-token", 0.89)),
        }
        assert!(!authorized(&target, &e), "arm {arm}");
    }
    let mut malformed = target.clone();
    malformed.prompt.options[0].value = malformed.choice.clone();
    assert!(!authorized(&malformed, &evidence()));
    malformed = target.clone();
    malformed.step.clear();
    assert!(!authorized(&malformed, &evidence()));
    for confidence in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
        let mut e = evidence();
        e.confidence = confidence;
        assert!(!authorized(&target, &e));
    }
}

#[test]
fn declared_option_response_refuses_multi_select_and_more_than_ten_choices() {
    let mut target = target();
    target.prompt.multi_select = Some(true);
    assert!(!unique_choice(&target.prompt, &target.choice));

    target.prompt.multi_select = Some(false);
    for index in 0..9 {
        target.prompt.options.push(PromptOption {
            label: format!("opaque-{index}"),
            value: format!("opaque-{index}"),
        });
    }
    assert_eq!(target.prompt.options.len(), 11);
    assert!(!unique_choice(&target.prompt, &target.choice));
}

#[test]
fn supporter_first_opinion_safe_response_distinct_events_and_new_occurrence() {
    let mut book = Agreement::default();
    let deadline = Instant::now() + Duration::from_secs(3);
    assert_eq!(
        book.observe("launch-a", "occ-1", 10, "step-1", 0.97, deadline),
        (1, 0.97)
    );
    assert_eq!(
        book.observe("launch-a", "occ-1", 10, "step-1", 0.99, deadline),
        (1, 0.97)
    );
    assert_eq!(
        book.observe("launch-a", "occ-1", 9, "step-1", 0.99, deadline),
        (1, 0.97)
    );
    assert_eq!(
        book.observe("launch-a", "occ-1", 12, "step-1", 0.93, deadline),
        (2, 0.93)
    );
    assert_eq!(
        book.observe("launch-a", "occ-2", 13, "step-1", 0.95, deadline),
        (1, 0.95)
    );
    assert_eq!(
        book.observe("launch-b", "occ-2", 14, "step-1", 0.96, deadline),
        (1, 0.96)
    );
    assert_eq!(
        book.observe("launch-b", "occ-2", 15, "install", 0.99, deadline),
        (1, 0.99)
    );
}

#[test]
fn supporter_first_opinion_safe_response_static_frame_distinct_events_confirm() {
    let mut book = Agreement::default();
    let deadline = Instant::now() + Duration::from_secs(3);
    assert_eq!(
        book.observe("launch", "occurrence", 31, "skip-token", 0.96, deadline),
        (1, 0.96)
    );
    assert_eq!(
        book.observe("launch", "occurrence", 32, "skip-token", 0.94, deadline),
        (2, 0.94)
    );
    assert_eq!(
        book.observe("launch", "occurrence", 32, "skip-token", 0.99, deadline),
        (2, 0.94)
    );
}

#[test]
fn supporter_first_opinion_safe_response_confirmation_expiry_and_step_change_reset() {
    let mut book = Agreement::default();
    assert_eq!(
        book.observe(
            "launch",
            "occurrence",
            51,
            "step-a",
            0.96,
            Instant::now() - Duration::from_millis(1),
        ),
        (1, 0.96)
    );
    assert_eq!(
        book.observe(
            "launch",
            "occurrence",
            52,
            "step-a",
            0.94,
            Instant::now() + Duration::from_secs(3),
        ),
        (1, 0.94)
    );
    assert_eq!(
        book.observe(
            "launch",
            "occurrence",
            53,
            "step-b",
            0.93,
            Instant::now() + Duration::from_secs(3),
        ),
        (1, 0.93)
    );
}

#[test]
fn supporter_first_opinion_confirmation_seed_and_child_are_one_shot_per_step() {
    let owner = coordinator::ActionOwner::default();
    assert!(owner.claim_followup("tab", "step-a", false));
    assert!(!owner.claim_followup("tab", "step-a", false));
    assert!(owner.claim_followup("tab", "step-a", true));
    assert!(!owner.claim_followup("tab", "step-a", true));
    assert!(owner.claim_followup("tab", "step-b", false));
    assert!(owner.claim_followup("tab", "step-b", true));
}

#[test]
fn supporter_first_opinion_safe_response_provider_option_order_and_focus() {
    use codeterm_plugin::provider::AgentProvider;
    let provider = codeterm_plugin::agent_claude::ClaudeProvider::new();
    let screen = include_str!("../../../../../plugins/provider-claude/tests/fixtures/onboarding/auto-environment-captured.txt");
    let initial = provider_target(&provider, screen).unwrap();
    assert_eq!(initial.kind, Kind::Onboarding);
    assert_eq!(initial.choice, "2");
    assert_eq!(initial.step, b"j");
    let focused = include_str!("../../../../../plugins/provider-claude/tests/fixtures/onboarding/auto-environment-not-now.txt");
    let focused_target = provider_target(&provider, focused).unwrap();
    assert_eq!(focused_target.choice, initial.choice);
    assert_eq!(focused_target.step, b"\r");
    assert_ne!(focused_target.frame_key, initial.frame_key);
    assert_eq!(
        provider.launch_onboarding_safe_choice("unrelated input"),
        None
    );
    assert!(provider_target(&provider, "unrelated input").is_none());
}

#[test]
fn sentinel_respond_prompt_onboarding_uses_declared_provider_choice() {
    let provider = codeterm_plugin::agent_claude::ClaudeProvider::new();
    let screen = include_str!("../../../../../plugins/provider-claude/tests/fixtures/onboarding/auto-environment-captured.txt");
    let target = provider_target(&provider, screen).unwrap();
    assert_eq!(target.kind, Kind::Onboarding);
    assert!(target.kind.label_matches(LOGIN_OR_AUTH_REQUIRED));
    assert!(!target.kind.label_matches(MODEL_UNAVAILABLE_OR_QUOTA));
    assert!(!target.kind.label_matches(TRUST_OR_PERMISSION_PROMPT));
    assert!(target
        .kind
        .sentinel_label_matches(TRUST_OR_PERMISSION_PROMPT));
    assert!(!target.kind.sentinel_label_matches(LOGIN_OR_AUTH_REQUIRED));
    assert!(!target
        .kind
        .sentinel_label_matches(MODEL_UNAVAILABLE_OR_QUOTA));
    assert!(authorized(
        &target,
        &Evidence {
            observations: 2,
            confidence: 0.99,
            qualified: true,
            current: true,
            eligible: true,
            model_choice: Some((&target.choice, 0.99)),
        }
    ));
}

#[test]
fn sentinel_respond_prompt_login_and_quota_are_never_authorized() {
    for kind in [
        Kind::Update,
        Kind::Onboarding,
        Kind::DirectoryTrust,
        Kind::Permission,
    ] {
        assert!(!kind.sentinel_label_matches(LOGIN_OR_AUTH_REQUIRED));
        assert!(!kind.sentinel_label_matches(MODEL_UNAVAILABLE_OR_QUOTA));
    }
    for kind in [Kind::Update, Kind::DirectoryTrust, Kind::Permission] {
        assert!(!kind.label_matches(LOGIN_OR_AUTH_REQUIRED));
        assert!(!kind.label_matches(MODEL_UNAVAILABLE_OR_QUOTA));
    }
    let provider = codeterm_plugin::agent_claude::ClaudeProvider::new();
    let login = include_str!(
        "../../../../../plugins/provider-claude/tests/fixtures/tab-state/auth-login.txt"
    );
    assert!(provider_target(&provider, login).is_none());
}

#[test]
fn supporter_first_opinion_safe_response_update_skip_order_focus_and_opaque_values() {
    let provider = codeterm_plugin::agent_claude::ClaudeProvider::new();
    for (screen, choice, step) in [
        (
            "Fixture selection\n❯ 1. Update\n 2. Skip\nEnter to confirm\n",
            "2",
            b"j".as_slice(),
        ),
        (
            "Fixture selection\n 1. Update\n❯ 2. Skip\nEnter to confirm\n",
            "2",
            b"\r".as_slice(),
        ),
        (
            "Fixture selection\n 1. Skip\n❯ 2. Update\nEnter to confirm\n",
            "1",
            b"k".as_slice(),
        ),
    ] {
        let target = provider_target(&provider, screen).unwrap();
        assert_eq!(target.kind, Kind::Update);
        assert_eq!(target.choice, choice);
        assert_eq!(target.step, step);
    }
    let mut update = target().prompt;
    update.options[0].label = "Update".into();
    update.options[1].label = "Skip".into();
    let provider = crate::agent::test_provider::MockProvider::new("opaque-provider")
        .with_parsed_prompt(update.clone())
        .with_classified_prompt_auto_response(
            codeterm_plugin::provider::ClassifiedPromptOptionRole::Skip,
        );
    use codeterm_plugin::provider::AgentProvider;
    assert_eq!(
        provider.update_prompt_safe_choice(&update),
        Some("skip-token".into())
    );
    assert_eq!(provider.live_prompt_response(1, "opaque-question\n"), None);
    assert!(provider_target(&provider, "opaque-question\n").is_none());
}

#[test]
fn supporter_first_opinion_safe_response_historical_focus_cannot_supply_update_enter() {
    let provider = codeterm_plugin::agent_claude::ClaudeProvider::new();
    let screen = "Previous selection\n 1. Update\n❯ 2. Skip\nCompleted previous choice\n\n\nFixture selection\n❯ 1. Update\n 2. Skip\nEnter to confirm\n";
    let target = provider_target(&provider, screen).unwrap();
    assert_eq!(target.choice, "2");
    assert_eq!(target.step, b"j");
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn supporter_first_opinion_safe_response_post_launch_planner_and_owner_boundary() {
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    mux.link_pty(&id, "safe-response-pty".into());
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("claude".into());
    tab.agent_session_id = Some("authenticated-session".into());
    tab.session_binding_nonce = Some("authenticated-nonce".into());
    tab.reported_session_binding_nonce = Some("authenticated-nonce".into());
    tab.role = Some(codeterm_types::agent::AgentRole::Manager);
    tab.role_profile = Some("planner".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        codeterm_plugin::agent_claude::ClaudeProvider::new(),
    ));
    let screen = include_str!("../../../../../plugins/provider-claude/tests/fixtures/onboarding/auto-environment-captured.txt");
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&id)
        .unwrap()
        .launch_session_authenticated());
    assert!(fixture
        .service
        .model_safe_response_target(&id, screen)
        .is_some());
    fixture.service.mux.write().get_tab_mut(&id).unwrap().role = None;
    assert!(fixture
        .service
        .model_safe_response_target(&id, screen)
        .is_none());
    assert_eq!(
        crate::service::output::prompt_step_dispatch_count_for_test("safe-response-pty"),
        0
    );
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&id)
        .unwrap()
        .pending_prompt
        .is_none());
}

#[test]
fn supporter_first_opinion_safe_response_qualification_is_question_set_specific() {
    assert_ne!(criteria_version(), super::super::criteria_version());
    let receipt = recorded_qualification().unwrap();
    assert!(!receipt.authorizes(
        "fixture",
        Some("fixture-model"),
        &criteria_version(),
        &"a".repeat(64)
    ));
}

#[test]
fn supporter_first_opinion_safe_response_adapter_replacement_resets_agreement() {
    struct Model;
    impl DecisionModel for Model {
        fn id(&self) -> &str {
            "fixture"
        }
        fn token_mode(&self) -> codeterm_types::decision::TokenMode {
            codeterm_types::decision::TokenMode::Required
        }
        fn primitives(&self) -> codeterm_types::decision::DecisionPrimitives {
            codeterm_types::decision::DecisionPrimitives {
                choice: true,
                noul: true,
                score: false,
            }
        }
        fn decide(
            &self,
            _: &codeterm_types::decision::DecisionRequest,
            _: Instant,
        ) -> Result<DecisionAnswer, codeterm_types::decision::DecisionError> {
            Err(codeterm_types::decision::DecisionError::Unsupported)
        }
    }
    let first: Arc<dyn DecisionModel> = Arc::new(Model);
    let second: Arc<dyn DecisionModel> = Arc::new(Model);
    let mut book = Agreement::default();
    let deadline = Instant::now() + Duration::from_secs(3);
    book.bind_adapter(&first);
    assert_eq!(
        book.observe("launch", "occ", 1, "step", 0.95, deadline),
        (1, 0.95)
    );
    book.bind_adapter(&first);
    assert_eq!(
        book.observe("launch", "occ", 2, "step", 0.95, deadline),
        (2, 0.95)
    );
    book.bind_adapter(&second);
    assert_eq!(
        book.observe("launch", "occ", 3, "step", 0.95, deadline),
        (1, 0.95)
    );
    assert_eq!(
        book.observe("launch", "occ", 4, "changed-policy:step", 0.95, deadline),
        (1, 0.95)
    );
}

#[test]
fn supporter_first_opinion_safe_response_same_option_new_occurrence_shares_trust_ledger() {
    let tab = uuid::Uuid::new_v4().to_string();
    let frame = "opaque-focus-frame";
    assert!(
        super::super::super::directory_trust::claim_model_onboarding_step(&tab, "launch:1", frame)
    );
    assert!(
        !super::super::super::directory_trust::claim_model_onboarding_step(&tab, "launch:1", frame)
    );
    super::super::super::directory_trust::finish_model_onboarding_step(
        &tab, "launch:1", frame, true,
    );
    assert!(
        !super::super::super::directory_trust::claim_model_onboarding_step(&tab, "launch:1", frame)
    );
    assert!(
        super::super::super::directory_trust::claim_model_onboarding_step(&tab, "launch:2", frame)
    );
    super::super::super::directory_trust::finish_model_onboarding_step(
        &tab, "launch:2", frame, true,
    );
}
