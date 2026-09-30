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
fn detected_prompt_stays_respondable_without_notification() {
    let screen =
        std::fs::read_to_string(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
            "../plugins/provider-claude/tests/fixtures/folder-trust/quick-safety-no-first.txt",
        ))
        .expect("Claude trust fixture");
    let (fixture, tab_id, pty_id) = fixture_with_screen(&screen);
    fixture.service.clear_pending_prompt(&tab_id);
    fixture.service.prompt_detector.lock().clear(&tab_id);
    let parsed = fixture
        .service
        .agent_registry
        .parse_interactive_prompt("claude", &screen)
        .expect("classified trust choices");
    let (prompt, kind) = parsed;
    assert_eq!(kind, codeterm_types::pty::PromptKind::DirectoryTrust);
    let decision = fixture.service.prompt_detector.lock().evaluate_pty_output_parsed(
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
    fixture.service.apply_prompt_decision_with_kind(
        &decision,
        Some(&screen),
        kind,
        true,
    );
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .and_then(|tab| tab.pending_prompt.as_ref()),
        Some(&prompt)
    );
    assert!(fixture.service.notif.lock().list(false).is_empty());

    let identity = codeterm_types::pty::PromptIdentity::new(kind, &prompt);
    let actions = prompt
        .options
        .iter()
        .map(|option| codeterm_types::notification::NotificationAction::RespondPromptOption {
            label: option.label.clone(),
            value: option.value.clone(),
            identity: identity.clone(),
        })
        .collect();
    let existing_id = fixture.service.create_prompt_notification_ext(
        "opaque title".into(),
        Some("opaque body".into()),
        Some("warning"),
        Some(actions),
        Some(true),
        crate::service::notification::NotificationOrigin::local_tab(tab_id.clone()),
        crate::service::notification::PushIntent {
            level: Some(crate::notification::push::NotifyLevel::ActionNeeded),
            event_key: Some("existing-prompt-condition".into()),
        },
    );
    assert_eq!(
        fixture.service.respond_prompt(&tab_id, "1"),
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
    let notifications = fixture.service.notif.lock();
    let existing = notifications
        .notification(&existing_id)
        .expect("preexisting record remains stored");
    assert!(existing.resolved);
    assert!(existing.read);
    assert!(existing.actions.is_none());
    let answer = existing.answer.as_ref().expect("answer slot");
    assert_eq!(answer.answered_via.as_deref(), Some("tab_respond"));
    assert!(answer.delivered);
    assert_eq!(notifications.list(false).len(), 1);
    drop(notifications);
    let _ = fixture.service.pty.lock().kill(&pty_id);
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime and live PTY")]
#[test]
fn tab_respond_answers_trust_prompt_in_pair_tabs() {
    let screen =
        std::fs::read_to_string(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
            "../plugins/provider-claude/tests/fixtures/folder-trust/quick-safety-no-first.txt",
        ))
        .expect("Claude trust fixture");
    let (fixture, tab_id, pty_id) = fixture_with_screen(&screen);
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).expect("pair Planner tab");
        tab.role = Some(crate::multiplexer::types::AgentRole::Manager);
        tab.role_profile = Some("planner".into());
        tab.team_id = Some("team-pair".into());
        tab.parent_tab_id = Some("orchestrator-tab".into());
    }
    let (prompt, kind) = fixture
        .service
        .agent_registry
        .parse_interactive_prompt("claude", &screen)
        .expect("classified trust choices");
    fixture.service.clear_pending_prompt(&tab_id);
    fixture.service.prompt_detector.lock().clear(&tab_id);
    fixture.service.apply_prompt_decision_with_kind(
        &crate::agent::prompt::PromptDecision::NewPrompt {
            tab_id: tab_id.clone(),
            prompt,
        },
        Some(&screen),
        kind,
        true,
    );
    assert!(fixture.service.notif.lock().list(false).is_empty());
    assert_eq!(
        fixture.service.respond_prompt(&tab_id, "1"),
        Ok(RespondPromptOutcome::Sent)
    );
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
