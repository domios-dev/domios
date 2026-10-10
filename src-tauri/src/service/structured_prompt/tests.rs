//! Store-level tests for the structured-prompt host module.

use super::opencode_sse::{
    parse_event, ChatPartUpdate, MessageMeta, ParsedEvent, PromptKind, ResponseRoute,
};
use super::*;
use crate::multiplexer::types::PendingPrompt;
use serde_json::Value;

mod model_tests;

fn dummy_prompt(q: &str) -> PendingPrompt {
    PendingPrompt {
        question: q.to_string(),
        options: Vec::new(),
        sections: None,
        multi_select: Some(false),
        submit_available: Some(true),
        selected_values: Some(Vec::new()),
    }
}

#[test]
fn rejected_question_clears_only_the_matching_pending_request() {
    let mut active = HashMap::new();
    for (tab_id, request_id) in [("owner", "que_owner"), ("other", "que_other")] {
        active.insert(
            tab_id.to_string(),
            Active {
                prompt: dummy_prompt(tab_id),
                in_flight: false,
                route: ResponseRoute {
                    session_id: format!("ses_{tab_id}"),
                    request_id: request_id.into(),
                    kind: PromptKind::Question,
                    option_labels: vec!["A".into()],
                    questions: vec![super::questions::Question {
                        header: "H".into(),
                        text: "Q".into(),
                        labels: vec!["A".into()],
                        multiple: false,
                        custom: false,
                    }],
                    answers: Vec::new(),
                },
            },
        );
    }
    let event = serde_json::json!({
        "type": "question.rejected",
        "properties": {"sessionID": "ses_owner", "requestID": "que_owner"}
    });
    let ParsedEvent::Replied { request_id } = parse_event(&event) else {
        panic!("expected prompt cancellation");
    };
    assert!(!clear_request(&mut active, "other", &request_id));
    assert!(!clear_request(&mut active, "owner", "que_stale"));
    assert_eq!(active.len(), 2);
    assert!(clear_request(&mut active, "owner", &request_id));
    assert!(!active.contains_key("owner"));
    assert_eq!(active["other"].route.request_id, "que_other");
    assert!(!clear_request(&mut active, "owner", &request_id));
}

#[test]
fn endpoint_and_active_round_trip_per_tab() {
    let tab = "test-tab-sp-roundtrip";
    register_endpoint(tab, "http://127.0.0.1:55555".into(), "/work".into());
    assert!(pending_for_tab(tab).is_none(), "no prompt yet");

    ACTIVE.lock().insert(
        tab.to_string(),
        Active {
            prompt: dummy_prompt("Pick one"),
            in_flight: false,
            route: ResponseRoute {
                session_id: "ses_1".into(),
                request_id: "que_1".into(),
                kind: PromptKind::Question,
                option_labels: vec!["A".into(), "B".into()],
                questions: vec![super::questions::Question {
                    header: "H".into(),
                    text: "Q".into(),
                    labels: vec!["A".into(), "B".into()],
                    multiple: false,
                    custom: false,
                }],
                answers: Vec::new(),
            },
        },
    );
    assert_eq!(pending_for_tab(tab).unwrap().question, "Pick one");
    assert_eq!(route_for_tab(tab).unwrap().request_id, "que_1");

    clear_tab(tab);
    assert!(pending_for_tab(tab).is_none());
    assert!(route_for_tab(tab).is_none());
    assert!(super::super::structured_endpoint::get(tab).is_none());
}

#[test]
fn allocate_server_returns_loopback_port() {
    let server = allocate_server().expect("port allocated");
    assert_eq!(server.hostname, "127.0.0.1");
    assert!(server.port > 0);
}

#[test]
fn pending_for_unknown_tab_is_none() {
    assert!(pending_for_tab("test-tab-sp-never-registered").is_none());
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn structured_multi_select_advances_without_a_pty_or_clearing_the_next_prompt() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_session_id = Some("own".into());
    tab.agent_type = Some("fixture".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let event = serde_json::json!({"type":"question.asked", "properties":{
        "id":"transaction", "sessionID":"own", "questions":[
            {"question":"same text", "multiple":true, "options":[{"label":"A"},{"label":"B"}]},
            {"question":"same text", "custom":false, "options":[{"label":"C"}]}
        ]
    }});
    let mut state = ChatStreamState::default();
    handle_event(&fixture.service, &tab_id, &event, &mut state);
    assert_eq!(
        fixture.service.respond_prompt(&tab_id, "10003").unwrap(),
        crate::service::tab::RespondPromptOutcome::Structured
    );
    let next = pending_for_tab(&tab_id).unwrap();
    assert_eq!(next.options[0].label, "C");
    assert_eq!(
        route_for_tab(&tab_id).unwrap().answers,
        vec![vec!["A", "B"]]
    );
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .pending_prompt
            .as_ref(),
        Some(&next)
    );
    handle_event(&fixture.service, &tab_id, &event, &mut state);
    assert_eq!(pending_for_tab(&tab_id).unwrap(), next);
    clear_tab(&tab_id);
}

#[test]
fn promotion_preserves_an_already_active_request() {
    let tab_id = "test-structured-promotion";
    let active = Active {
        prompt: dummy_prompt("queued"),
        route: ResponseRoute {
            session_id: "own".into(),
            request_id: "queued".into(),
            kind: PromptKind::Permission,
            option_labels: vec!["once".into()],
            questions: Vec::new(),
            answers: Vec::new(),
        },
        in_flight: false,
    };
    QUEUED.lock().insert(tab_id.into(), vec![active]);
    assert_eq!(promote_prompt(tab_id).unwrap().question, "queued");
    assert_eq!(route_for_tab(tab_id).unwrap().request_id, "queued");
    assert_eq!(promote_prompt(tab_id).unwrap().question, "queued");
    assert!(QUEUED.lock()[tab_id].is_empty());
    clear_tab(tab_id);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn structured_prompt_notification_answer_records_tab_respond_metadata() {
    let (mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let prompt = PendingPrompt {
        question: "Proceed?".into(),
        options: vec![
            crate::multiplexer::types::PromptOption {
                label: "Proceed".into(),
                value: "yes".into(),
            },
            crate::multiplexer::types::PromptOption {
                label: "Cancel".into(),
                value: "no".into(),
            },
        ],
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    };
    let identity = codeterm_types::pty::PromptIdentity::new(
        codeterm_types::pty::PromptKind::Structured,
        &prompt,
    );
    let notification_id = fixture.service.create_prompt_notification_ext(
        "Prompt".into(),
        Some("structured prompt".into()),
        Some("warning"),
        Some(vec![
            codeterm_types::notification::NotificationAction::RespondPromptOption {
                label: "Proceed".into(),
                value: "yes".into(),
                identity: identity.clone(),
            },
            codeterm_types::notification::NotificationAction::RespondPromptOption {
                label: "Cancel".into(),
                value: "no".into(),
                identity: identity.clone(),
            },
        ]),
        Some(true),
        crate::service::notification::NotificationOrigin::local_tab(tab_id.clone()),
        crate::service::notification::PushIntent {
            level: Some(crate::notification::push::NotifyLevel::ActionNeeded),
            event_key: Some("structured-prompt-answer-test".into()),
        },
    );

    fixture
        .service
        .answer_prompt_notifications(&tab_id, &identity, 1, "tab_respond");
    let store = fixture.service.notif.lock();
    let notification = store.notification(&notification_id).unwrap();
    assert_eq!(
        notification.answer.as_ref().unwrap().answered.as_deref(),
        Some("Cancel")
    );
    assert_eq!(
        notification
            .answer
            .as_ref()
            .unwrap()
            .answered_via
            .as_deref(),
        Some("tab_respond")
    );
    assert!(notification.resolved);
    assert!(notification.read);
    assert!(notification.actions.is_none());
    assert!(notification.answer.as_ref().unwrap().delivered);
}

#[test]
fn opencode_reasoning_delta_after_typed_update_emits_thinking() {
    let mut state = ChatStreamState::default();
    let message = captured_message();
    let typed = captured_part(include_str!(
        "fixtures/captured_message_part_reasoning_empty.json"
    ));
    let delta = captured_part(include_str!(
        "fixtures/captured_message_part_delta_first.json"
    ));

    assert!(state.prepare_chat_part(&typed, &message).is_none());
    assert_eq!(
        state.part_kind.get("prt_text").map(String::as_str),
        Some("reasoning")
    );

    let emitted = state.prepare_chat_part(&delta, &message).unwrap();
    assert_eq!(emitted.kind, "thinking");
    assert_eq!(emitted.content, "121");
}

#[test]
fn opencode_delta_before_any_update_is_held_until_typed() {
    let mut state = ChatStreamState::default();
    let message = captured_message();
    let delta = captured_part(include_str!(
        "fixtures/captured_message_part_delta_first.json"
    ));
    let typed = captured_part(include_str!(
        "fixtures/captured_message_part_reasoning_empty.json"
    ));

    assert!(state.prepare_chat_part(&delta, &message).is_none());
    assert_eq!(
        state.part_text.get("prt_text").map(String::as_str),
        Some("121")
    );
    assert!(state.part_kind.is_empty());

    let emitted = state.prepare_chat_part(&typed, &message).unwrap();
    assert_eq!(emitted.kind, "thinking");
    assert_eq!(emitted.content, "121");
}

#[test]
fn opencode_part_kind_is_cleared_with_part_text() {
    let mut state = ChatStreamState::default();
    state.reset_session("ses_1");
    state.part_text.insert("part_old".into(), "buffered".into());
    state
        .part_kind
        .insert("part_old".into(), "reasoning".into());
    state.reset_session("ses_2");
    assert!(state.part_text.is_empty());
    assert!(state.part_kind.is_empty());

    state.reset_session("ses_1");
    for index in 0..512 {
        let id = format!("message-{index}");
        state.messages.insert(
            id.clone(),
            MessageMeta {
                model: None,
                session_id: "ses_1".into(),
                message_id: id.clone(),
                role: "assistant".into(),
                timestamp: None,
            },
        );
        state.part_text.insert(id.clone(), "text".into());
        state.part_kind.insert(id, "text".into());
    }
    state.record_message(MessageMeta {
        model: None,
        session_id: "ses_1".into(),
        message_id: "message-new".into(),
        role: "assistant".into(),
        timestamp: None,
    });
    assert_eq!(state.messages.len(), 1);
    assert!(state.part_text.is_empty());
    assert!(state.part_kind.is_empty());

    for index in 0..512 {
        state
            .part_text
            .insert(format!("part-{index}"), "text".into());
    }
    state
        .part_kind
        .insert("prt_text".into(), "reasoning".into());
    let delta = captured_part(include_str!(
        "fixtures/captured_message_part_delta_first.json"
    ));
    let emitted = state
        .prepare_chat_part(&delta, &captured_message())
        .unwrap();
    assert_eq!(emitted.kind, "thinking");
    assert_eq!(
        state.part_text.get("prt_text").map(String::as_str),
        Some("121")
    );
    assert_eq!(state.part_kind.len(), 1);
    assert_eq!(
        state.part_kind.get("prt_text").map(String::as_str),
        Some("reasoning")
    );

    state.part_text.clear();
    state.part_kind.clear();
    for index in 0..512 {
        state
            .part_kind
            .insert(format!("part-{index}"), "tool".into());
    }
    let typed = captured_part(include_str!(
        "fixtures/captured_message_part_reasoning_empty.json"
    ));
    assert!(state
        .prepare_chat_part(&typed, &captured_message())
        .is_none());
    assert!(state.part_text.is_empty());
    assert_eq!(state.part_kind.len(), 1);
    assert_eq!(
        state.part_kind.get("prt_text").map(String::as_str),
        Some("reasoning")
    );
}

fn captured_message() -> MessageMeta {
    let frame: Value =
        serde_json::from_str(include_str!("fixtures/captured_message_updated.json")).unwrap();
    let ParsedEvent::MessageUpdated { message } = parse_event(&frame) else {
        panic!("expected message metadata");
    };
    message
}

fn captured_part(frame: &str) -> ChatPartUpdate {
    let frame: Value = serde_json::from_str(frame).unwrap();
    let ParsedEvent::PartUpdated { update } = parse_event(&frame) else {
        panic!("expected part update");
    };
    update
}
