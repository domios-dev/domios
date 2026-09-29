use super::*;
use crate::multiplexer::types::{PendingPrompt, PromptOption};
use std::path::PathBuf;

fn pending_prompt() -> PendingPrompt {
    PendingPrompt {
        question: "Choose an option".into(),
        options: vec![
            PromptOption {
                label: "No".into(),
                value: "1".into(),
            },
            PromptOption {
                label: "Yes".into(),
                value: "2".into(),
            },
        ],
        sections: None,
        multi_select: Some(false),
        submit_available: Some(true),
        selected_values: None,
    }
}

fn fixture_with_screen(
    screen: &str,
) -> (crate::service::test_fixture::ServiceFixture, String, String) {
    use crate::multiplexer::state::MultiplexerState;

    let (mut mux, tab_id) = MultiplexerState::new();
    let pty_id = format!("respond-prompt-{}", uuid::Uuid::new_v4());
    assert_eq!(mux.link_pty(&tab_id, pty_id.clone()), None);
    let prompt = pending_prompt();
    let tab = mux.get_tab_mut(&tab_id).expect("tab");
    tab.agent_type = Some("claude".into());
    tab.pending_prompt = Some(prompt.clone());
    tab.pending_prompt_kind = Some(codeterm_types::pty::PromptKind::Provider);
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));
    fixture
        .service
        .prompt_detector
        .lock()
        .hook_detected_scraped(&tab_id, prompt);
    fixture
        .service
        .output_buf
        .lock()
        .append(&pty_id, screen.as_bytes());
    fixture
        .service
        .pty
        .lock()
        .spawn_with_id(Some(pty_id.clone()), None, None, 80, 24, Default::default())
        .expect("test PTY");
    (fixture, tab_id, pty_id)
}

fn fixture_with_pending_prompt() -> (crate::service::test_fixture::ServiceFixture, String) {
    use crate::multiplexer::state::MultiplexerState;

    let (mut mux, tab_id) = MultiplexerState::new();
    let prompt = pending_prompt();
    let tab = mux.get_tab_mut(&tab_id).expect("tab");
    tab.agent_type = Some("claude".into());
    tab.pending_prompt = Some(prompt.clone());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture
        .service
        .prompt_detector
        .lock()
        .hook_detected_scraped(&tab_id, prompt);
    (fixture, tab_id)
}

fn has_input_events(cast: PathBuf) -> bool {
    std::fs::read_to_string(cast)
        .expect("capture file")
        .lines()
        .skip(1)
        .map(serde_json::from_str::<serde_json::Value>)
        .collect::<Result<Vec<_>, _>>()
        .expect("capture events")
        .iter()
        .any(|event| event.get(1).and_then(serde_json::Value::as_str) == Some("i"))
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime and live PTY")]
#[test]
fn tab_respond_marks_trust_prompt_notification_answered() {
    tab_respond_marks_prompt_notification_answered();
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime and live PTY")]
#[test]
fn tab_respond_answers_trust_prompt_in_pair_tabs() {
    let screen =
        std::fs::read_to_string(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
            "../plugins/provider-claude/tests/fixtures/folder-trust/quick-safety-no-first.txt",
        ))
        .expect("Claude trust fixture");
    let (fixture, tab_id, _) = fixture_with_screen(&screen);
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).expect("pair Planner tab");
        tab.role = Some(crate::multiplexer::types::AgentRole::Manager);
        tab.role_profile = Some("planner".into());
        tab.team_id = Some("team-pair".into());
        tab.parent_tab_id = Some("orchestrator-tab".into());
    }
    let parsed = fixture
        .service
        .agent_registry
        .parse_interactive_prompt("claude", &screen)
        .expect("classified trust choices");
    assert_eq!(parsed.1, codeterm_types::pty::PromptKind::DirectoryTrust);
    let affirmative = parsed
        .0
        .options
        .iter()
        .position(|option| option.label.to_lowercase().starts_with("yes"))
        .expect("affirmative trust choice");
    fixture.service.clear_pending_prompt(&tab_id);
    fixture.service.prompt_detector.lock().clear(&tab_id);
    fixture.service.apply_prompt_decision_with_kind(
        &crate::agent::prompt::PromptDecision::NewPrompt {
            tab_id: tab_id.clone(),
            prompt: parsed.0,
        },
        Some(&screen),
        codeterm_types::pty::PromptKind::DirectoryTrust,
        true,
    );
    assert_eq!(
        fixture
            .service
            .respond_prompt(&tab_id, &(affirmative + 1).to_string()),
        Ok(RespondPromptOutcome::Sent)
    );
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime and live PTY")]
#[test]
fn unstructured_respond_also_resolves_its_prompt_notification() {
    tab_respond_marks_prompt_notification_answered();
}

fn tab_respond_marks_prompt_notification_answered() {
    let screen = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../plugins/provider-claude/tests/fixtures/folder-trust")
            .join("quick-safety-no-first.txt"),
    )
    .expect("captured folder-trust fixture");
    let (fixture, tab_id, pty_id) = fixture_with_screen(&screen);
    let prompt = pending_prompt();
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).expect("tab");
        tab.pending_prompt = None;
        tab.pending_prompt_kind = None;
    }
    fixture.service.apply_prompt_decision_with_kind(
        &crate::agent::prompt::PromptDecision::NewPrompt {
            tab_id: tab_id.clone(),
            prompt: prompt.clone(),
        },
        Some(&screen),
        codeterm_types::pty::PromptKind::DirectoryTrust,
        true,
    );
    let first_id = {
        let notifications = fixture.service.notif.lock();
        notifications
            .list(false)
            .into_iter()
            .find(|notification| notification.tab_id.as_deref() == Some(tab_id.as_str()))
            .expect("prompt notification")
            .id
            .clone()
    };
    let unrelated_id = fixture.service.create_prompt_notification_ext(
        "Unrelated prompt".into(),
        Some("unrelated".into()),
        Some("warning"),
        Some(vec![
            codeterm_types::notification::NotificationAction::RespondPromptOption {
                label: "Later".into(),
                value: "1".into(),
                identity: codeterm_types::pty::PromptIdentity::new(
                    codeterm_types::pty::PromptKind::Provider,
                    &PendingPrompt {
                        question: "unrelated".into(),
                        options: vec![crate::multiplexer::types::PromptOption {
                            label: "Later".into(),
                            value: "1".into(),
                        }],
                        sections: None,
                        multi_select: None,
                        submit_available: None,
                        selected_values: None,
                    },
                ),
            },
        ]),
        Some(true),
        crate::service::notification::NotificationOrigin::local_tab(tab_id.clone()),
        crate::service::notification::PushIntent {
            level: Some(crate::notification::push::NotifyLevel::ActionNeeded),
            event_key: Some("unrelated-prompt-answer-test".into()),
        },
    );
    assert_eq!(
        fixture
            .service
            .agent_registry
            .build_prompt_response("claude", 1, &screen),
        Some(vec![b"j".to_vec(), b"\r".to_vec()])
    );
    assert_eq!(
        fixture.service.respond_prompt(&tab_id, "2"),
        Ok(RespondPromptOutcome::Sent)
    );
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .unwrap()
        .pending_prompt
        .is_none());
    assert!(fixture
        .service
        .prompt_detector
        .lock()
        .active_prompt(&tab_id)
        .is_none());
    {
        let notifications = fixture.service.notif.lock();
        let first = notifications
            .notification(&first_id)
            .expect("resolved notification");
        assert!(first.resolved);
        assert!(first.read);
        assert!(first.actions.is_none());
        let answer = first.answer.as_ref().expect("answer slot");
        assert_eq!(answer.answered.as_deref(), Some("Yes"));
        assert_eq!(answer.answered_via.as_deref(), Some("tab_respond"));
        assert!(answer.delivered);
        let unrelated = notifications.notification(&unrelated_id).unwrap();
        assert!(!unrelated.resolved);
        assert_eq!(unrelated.answer.as_ref().unwrap().answered, None);
        assert!(unrelated.actions.is_some());
    }
    fixture.service.apply_prompt_decision_with_kind(
        &crate::agent::prompt::PromptDecision::NewPrompt {
            tab_id: tab_id.clone(),
            prompt,
        },
        Some(&screen),
        codeterm_types::pty::PromptKind::Provider,
        true,
    );
    let notifications = fixture.service.notif.lock();
    let for_tab = notifications
        .list(false)
        .into_iter()
        .filter(|notification| notification.tab_id.as_deref() == Some(tab_id.as_str()))
        .filter(|notification| notification.title != "Unrelated prompt")
        .collect::<Vec<_>>();
    assert_eq!(for_tab.len(), 2);
    assert_ne!(for_tab[0].id, for_tab[1].id);
    assert!(for_tab[0].resolved);
    assert!(!for_tab[1].resolved);
    assert!(for_tab[1]
        .actions
        .as_ref()
        .is_some_and(|actions| !actions.is_empty()));
    let _ = fixture.service.pty.lock().kill(&pty_id);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn respond_prompt_rejects_invalid_options_before_pty_access() {
    let (fixture, tab_id) = fixture_with_pending_prompt();
    for value in ["3", "two"] {
        assert_eq!(
            fixture.service.respond_prompt(&tab_id, value),
            Ok(RespondPromptOutcome::InvalidOption)
        );
    }
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .unwrap()
        .pending_prompt
        .is_some());
    assert!(fixture
        .service
        .prompt_detector
        .lock()
        .active_prompt(&tab_id)
        .is_some());
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime and live PTY")]
#[tokio::test]
async fn respond_prompt_reports_not_on_screen_instead_of_ok() {
    let (fixture, tab_id, pty_id) = fixture_with_screen("");
    let capture_path = fixture.temp.path().join("respond-prompt.cast");
    let capture = crate::pty::capture::CaptureRegistry::global();
    capture
        .start(&pty_id, capture_path, 80, 24, 0, Vec::new())
        .expect("capture input writes");

    let result = fixture.service.respond_prompt(&tab_id, "1");
    let cast = capture.stop(&pty_id).expect("capture stopped");
    let _ = fixture.service.pty.lock().kill(&pty_id);
    assert_eq!(result, Ok(RespondPromptOutcome::PromptNotOnScreen));
    assert!(!has_input_events(cast));
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .unwrap()
        .pending_prompt
        .is_some());
    assert!(fixture
        .service
        .prompt_detector
        .lock()
        .active_prompt(&tab_id)
        .is_some());
}

#[tokio::test]
async fn respond_prompt_outcome_maps_not_on_screen_to_conflict() {
    let response =
        crate::api::agent_routes::respond_prompt_response(RespondPromptOutcome::PromptNotOnScreen);
    assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).expect("response JSON"),
        serde_json::json!({"ok": false, "reason": "prompt_not_on_screen"})
    );

    let invalid =
        crate::api::agent_routes::respond_prompt_response(RespondPromptOutcome::InvalidOption);
    assert_eq!(invalid.status(), axum::http::StatusCode::BAD_REQUEST);
}
