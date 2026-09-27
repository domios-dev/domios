use super::*;
use crate::agent::status::TabStatus;
use crate::agent::tab_state::{
    DelayedResume, Surface, TabState, TabStateAction, TabStateClassification,
};
use crate::agent::test_provider::MockProvider;
use codeterm_types::pty::{PendingPrompt, PromptOption};

fn classification(
    state: TabState,
    confidence: f32,
    action: Option<TabStateAction>,
) -> TabStateClassification {
    TabStateClassification {
        state,
        confidence,
        data: None,
        action,
    }
}

fn keys_action(keys: &[&str]) -> TabStateAction {
    TabStateAction {
        keys: Some(keys.iter().map(|k| k.to_string()).collect()),
        delayed_resume: None,
        surface: None,
    }
}

#[test]
fn does_not_fire_while_working() {
    // Gate: even a confident, actionable verdict is ignored while streaming.
    let c = classification(
        TabState::UpdatePrompt,
        0.99,
        Some(keys_action(&["down", "enter"])),
    );
    assert_eq!(
        decide(TabStatus::Working, Some(&c)),
        ReactorDecision::Ignore
    );
}

#[test]
fn does_not_fire_when_dead() {
    let c = classification(TabState::UpdatePrompt, 0.99, Some(keys_action(&["enter"])));
    assert_eq!(decide(TabStatus::Dead, Some(&c)), ReactorDecision::Ignore);
}

#[test]
fn confidence_below_threshold_yields_no_action() {
    let c = classification(
        TabState::UpdatePrompt,
        GLOBAL_THRESHOLD - 0.01,
        Some(keys_action(&["down", "enter"])),
    );
    assert_eq!(decide(TabStatus::Idle, Some(&c)), ReactorDecision::Ignore);
}

#[test]
fn no_classification_yields_no_action() {
    assert_eq!(decide(TabStatus::Idle, None), ReactorDecision::Ignore);
    assert_eq!(decide(TabStatus::Waiting, None), ReactorDecision::Ignore);
}

#[test]
fn waiting_on_auth_is_never_auto_acted_surface_only() {
    // press a key — it surfaces only.
    let surface = Surface {
        kind: "auth".to_string(),
        payload: serde_json::json!({ "msg": "paste your token" }),
    };
    let action = TabStateAction {
        keys: Some(vec!["enter".to_string()]),
        delayed_resume: None,
        surface: Some(surface.clone()),
    };
    let c = classification(TabState::WaitingOnAuth, 0.99, Some(action));
    assert_eq!(
        decide(TabStatus::Idle, Some(&c)),
        ReactorDecision::Surface(Some(surface))
    );
}

#[test]
fn waiting_on_auth_with_no_surface_still_surfaces() {
    let action = keys_action(&["enter"]);
    let c = classification(TabState::WaitingOnAuth, 1.0, Some(action));
    assert_eq!(
        decide(TabStatus::Waiting, Some(&c)),
        ReactorDecision::Surface(None)
    );
}

#[test]
fn high_confidence_keys_action_is_executed() {
    let action = keys_action(&["down", "enter"]);
    let c = classification(TabState::IdleStuck, GLOBAL_THRESHOLD, Some(action.clone()));
    assert_eq!(
        decide(TabStatus::Idle, Some(&c)),
        ReactorDecision::Act(action)
    );
}

#[test]
fn high_confidence_action_with_keys_dispatches_key_injection_bytes() {
    // `send --actions` path injects: arrow-down then carriage return.
    let action = keys_action(&["down", "enter"]);
    let ReactorDecision::Act(decided) = decide(
        TabStatus::Waiting,
        Some(&classification(TabState::IdleStuck, 0.95, Some(action))),
    ) else {
        panic!("expected Act decision");
    };
    let writes = action_to_writes(&decided);
    assert_eq!(writes, vec![b"\x1b[B".to_vec(), b"\r".to_vec()]);
}

#[test]
fn action_with_no_keys_produces_no_writes() {
    let action = TabStateAction {
        keys: None,
        delayed_resume: Some(DelayedResume {
            at: "2026-06-19T16:20:00+03:00".to_string(),
            text: "continue your work".to_string(),
            then_keys: None,
        }),
        surface: None,
    };
    assert!(action_to_writes(&action).is_empty());
}

#[test]
fn unknown_keys_are_skipped_not_injected() {
    let action = keys_action(&["down", "frobnicate", "enter"]);
    let writes = action_to_writes(&action);
    assert_eq!(writes, vec![b"\x1b[B".to_vec(), b"\r".to_vec()]);
}

#[test]
fn update_prompt_classification_is_a_manual_prompt_guard() {
    let prompt = classification(TabState::UpdatePrompt, GLOBAL_THRESHOLD, None);
    let weak = classification(TabState::UpdatePrompt, GLOBAL_THRESHOLD - 0.01, None);
    let working = classification(TabState::Working, 1.0, None);
    assert!(is_manual_update_prompt(Some(&prompt)));
    assert!(!is_manual_update_prompt(Some(&weak)));
    assert!(!is_manual_update_prompt(Some(&working)));
    assert!(!is_manual_update_prompt(None));
}

#[test]
fn update_prompt_without_an_explicit_update_action_is_never_acted() {
    let prompt = classification(TabState::UpdatePrompt, 1.0, None);
    assert_eq!(
        decide(TabStatus::Idle, Some(&prompt)),
        ReactorDecision::Ignore
    );
    let keyed = classification(TabState::UpdatePrompt, 1.0, Some(keys_action(&["enter"])));
    assert_eq!(
        decide(TabStatus::Idle, Some(&keyed)),
        ReactorDecision::Ignore
    );
}

#[test]
fn rfc3339_reset_time_parses_to_epoch_ms() {
    // The +03:00 offset is honored: same instant as 13:20:00 UTC.
    let ms = parse_iso_to_ms("2026-06-19T16:20:00+03:00").expect("parse");
    let expected = chrono::DateTime::parse_from_rfc3339("2026-06-19T13:20:00Z")
        .unwrap()
        .timestamp_millis() as u64;
    assert_eq!(ms, expected);
}

#[test]
fn garbage_reset_time_is_rejected() {
    assert_eq!(parse_iso_to_ms("4:20pm (Europe/Riga)"), None);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn live_codex_trust_frame_becomes_pending_prompt_through_state_reactor() {
    let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../plugins/provider-codex/tests/fixtures/directory-trust/trust-frame-full-screen-trailing-blank-rows.txt");
    let screen = std::fs::read_to_string(fixture_path).expect("captured Codex folder trust frame");
    let options = screen
        .lines()
        .filter_map(|line| {
            let row = line
                .trim()
                .strip_prefix('›')
                .unwrap_or(line.trim())
                .trim_start();
            let digits = row.chars().take_while(char::is_ascii_digit).count();
            let value = row.get(..digits)?.parse::<usize>().ok()?;
            let label = row
                .get(digits..)?
                .trim_start()
                .strip_prefix('.')?
                .trim_start();
            (!label.is_empty()).then(|| PromptOption {
                label: label.to_string(),
                value: value.to_string(),
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        options.len(),
        2,
        "the live frame exposes both trust choices"
    );
    assert_eq!(
        options
            .iter()
            .map(|option| option.value.as_str())
            .collect::<Vec<_>>(),
        vec!["1", "2"]
    );
    let prompt = PendingPrompt {
        question: "folder access choice".into(),
        options,
        sections: None,
        multi_select: Some(false),
        submit_available: Some(false),
        selected_values: Some(vec!["1".into()]),
    };
    let provider = MockProvider::with_directory_trust_response(
        "trust-fixture",
        codeterm_plugin::provider::DirectoryTrustPromptResponse::Hold,
    )
    .with_tab_state(TabStateClassification {
        state: TabState::ClarifyingQuestion,
        confidence: 1.0,
        data: None,
        action: Some(TabStateAction {
            keys: None,
            delayed_resume: None,
            surface: Some(Surface {
                kind: "clarifying_question".into(),
                payload: serde_json::json!({"questions": ["folder access choice"]}),
            }),
        }),
    })
    .with_parsed_prompt(prompt.clone());
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let pty_id = "pty-live-trust-frame";
    mux.link_pty(&tab_id, pty_id.into());
    let tab = mux.get_tab_mut(&tab_id).expect("tab");
    tab.agent_type = Some("trust-fixture".into());
    tab.cwd = Some("C:\\Users\\dev\\workspace".into());
    tab.auto_approve = true;
    tab.permission_auto_confirm = true;

    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(provider));
    fixture
        .service
        .screen_state
        .lock()
        .process(pty_id, screen.as_bytes());
    let service = std::sync::Arc::new(fixture.service.clone());

    react(&service, pty_id, TabStatus::Waiting);

    let pending = fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .and_then(|tab| tab.pending_prompt.clone())
        .expect("the state reactor routes provider choices into tab prompt state");
    assert_eq!(pending, prompt);

    let tab_notifications = || {
        fixture
            .service
            .notif
            .lock()
            .list(false)
            .into_iter()
            .filter(|notification| notification.tab_id.as_deref() == Some(tab_id.as_str()))
            .cloned()
            .collect::<Vec<_>>()
    };
    let notifications = tab_notifications();
    assert_eq!(notifications.len(), 1);
    let notification = &notifications[0];
    let expected_body = std::iter::once(prompt.question.as_str())
        .chain(prompt.options.iter().map(|option| option.label.trim()))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(notification.body.as_deref(), Some(expected_body.as_str()));
    assert_eq!(
        notification.level,
        Some(crate::notification::push::NotifyLevel::ActionNeeded)
    );
    assert_eq!(
        notification
            .actions
            .as_ref()
            .expect("choice actions are offered")
            .iter()
            .filter_map(|action| action.answer_label())
            .collect::<Vec<_>>(),
        prompt
            .options
            .iter()
            .map(|option| option.label.trim())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        notification
            .answer
            .as_ref()
            .and_then(|answer| answer.asked_from_tab.as_deref()),
        Some(tab_id.as_str())
    );

    react(&service, pty_id, TabStatus::Waiting);
    assert_eq!(tab_notifications().len(), 1);
    fixture.service.apply_prompt_decision_with_screen(
        &crate::agent::prompt::PromptDecision::NewPrompt {
            tab_id: tab_id.clone(),
            prompt: prompt.clone(),
        },
        Some(&screen),
        false,
        true,
    );
    assert_eq!(tab_notifications().len(), 1);
    let mut selected_prompt = prompt.clone();
    selected_prompt.selected_values = Some(vec!["2".into()]);
    fixture.service.apply_prompt_decision_with_screen(
        &crate::agent::prompt::PromptDecision::PromptUpdated {
            tab_id: tab_id.clone(),
            prompt: selected_prompt,
        },
        Some(&screen),
        false,
        true,
    );
    assert_eq!(tab_notifications().len(), 1);

    let mut next_prompt = prompt.clone();
    next_prompt.question = "next folder access choice".into();
    fixture.service.prompt_detector.lock().clear(&tab_id);
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .expect("tab")
        .pending_prompt = None;
    fixture.service.agent_registry.register_provider(Box::new(
        MockProvider::with_directory_trust_response(
            "trust-fixture",
            codeterm_plugin::provider::DirectoryTrustPromptResponse::Hold,
        )
        .with_tab_state(TabStateClassification {
            state: TabState::ClarifyingQuestion,
            confidence: 1.0,
            data: None,
            action: Some(TabStateAction {
                keys: None,
                delayed_resume: None,
                surface: Some(Surface {
                    kind: "clarifying_question".into(),
                    payload: serde_json::json!({"questions": [next_prompt.question.clone()]}),
                }),
            }),
        })
        .with_parsed_prompt(next_prompt.clone()),
    ));
    react(&service, pty_id, TabStatus::Waiting);
    let notifications = tab_notifications();
    assert_eq!(notifications.len(), 2);
    assert!(notifications.iter().any(|notification| notification
        .body
        .as_deref()
        .is_some_and(|body| body.contains(&next_prompt.question))));
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn auto_approve_output_handles_permission_prompt_after_reactor_classification() {
    let prompt = PendingPrompt {
        question: "Allow this command?".into(),
        options: vec![
            PromptOption {
                label: "Yes, allow once".into(),
                value: "yes".into(),
            },
            PromptOption {
                label: "No".into(),
                value: "no".into(),
            },
        ],
        sections: None,
        multi_select: Some(false),
        submit_available: Some(false),
        selected_values: None,
    };
    let provider = MockProvider::new("permission-fixture")
        .with_tab_state(classification(
            TabState::ClarifyingQuestion,
            1.0,
            Some(keys_action(&["enter"])),
        ))
        .with_parsed_prompt(prompt.clone());
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let pty_id = "pty-auto-approve-output-path";
    mux.link_pty(&tab_id, pty_id.into());
    let tab = mux.get_tab_mut(&tab_id).expect("tab");
    tab.agent_type = Some("permission-fixture".into());
    tab.pty_id = Some(pty_id.into());
    tab.agent_session_id = Some("authenticated-session".into());
    tab.session_binding_nonce = Some("current-nonce".into());
    tab.reported_session_binding_nonce = Some("current-nonce".into());
    tab.auto_approve = true;
    assert!(tab.launch_session_authenticated());

    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(provider));
    fixture
        .service
        .screen_state
        .lock()
        .process(pty_id, b"Allow this command?\n1. Yes, allow once\n2. No\n");
    let service = std::sync::Arc::new(fixture.service.clone());

    react(&service, pty_id, TabStatus::Waiting);

    assert!(
        fixture
            .service
            .prompt_detector
            .lock()
            .active_prompt(&tab_id)
            .is_none(),
        "the reactor leaves this auto-approve prompt to the output path"
    );
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .and_then(|tab| tab.pending_prompt.as_ref())
        .is_none());

    let decision = fixture
        .service
        .prompt_detector
        .lock()
        .evaluate_pty_output_parsed(
            &tab_id,
            true,
            true,
            Some(crate::agent::prompt::ResolvedPrompt {
                prompt: prompt.clone(),
                explicit: false,
            }),
        );
    assert!(matches!(
        &decision,
        crate::agent::prompt::PromptDecision::NewPrompt { .. }
    ));
    fixture.service.apply_prompt_decision_with_screen(
        &decision,
        Some("Allow this command?\n1. Yes, allow once\n2. No\n"),
        false,
        false,
    );

    assert!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .and_then(|tab| tab.pending_prompt.as_ref())
            .is_none(),
        "the output path auto-approves instead of being starved by reactor state"
    );
    assert!(
        fixture
            .service
            .prompt_detector
            .lock()
            .active_prompt(&tab_id)
            .is_none(),
        "the handled prompt leaves active detection after output approval"
    );
}
