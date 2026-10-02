use super::*;
use codeterm_types::pty::PromptOption;

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
fn supporter_first_opinion_safe_response_distinct_events_and_new_occurrence() {
    let mut book = Agreement::default();
    assert_eq!(
        book.observe("launch-a", 1, 10, 100, "skip-token", 0.97),
        (1, 0.97)
    );
    assert_eq!(
        book.observe("launch-a", 1, 10, 101, "skip-token", 0.99),
        (1, 0.97)
    );
    assert_eq!(
        book.observe("launch-a", 1, 11, 100, "skip-token", 0.99),
        (1, 0.97)
    );
    assert_eq!(
        book.observe("launch-a", 1, 12, 102, "skip-token", 0.93),
        (2, 0.93)
    );
    assert_eq!(
        book.observe("launch-a", 2, 13, 103, "skip-token", 0.95),
        (1, 0.95)
    );
    assert_eq!(
        book.observe("launch-b", 2, 14, 104, "skip-token", 0.96),
        (1, 0.96)
    );
    assert_eq!(
        book.observe("launch-b", 2, 15, 105, "install", 0.99),
        (1, 0.99)
    );
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
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn supporter_first_opinion_safe_response_post_launch_planner_and_owner_boundary() {
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    mux.link_pty(&id, "safe-response-pty".into());
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("claude".into());
    tab.agent_session_id = Some("authenticated-session".into());
    tab.role = Some(codeterm_types::agent::AgentRole::Manager);
    tab.role_profile = Some("planner".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        codeterm_plugin::agent_claude::ClaudeProvider::new(),
    ));
    let screen = include_str!("../../../../../plugins/provider-claude/tests/fixtures/onboarding/auto-environment-captured.txt");
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
