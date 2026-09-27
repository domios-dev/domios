//! Store-level tests for the structured-prompt host module. The SSE parsing is
//! covered in `opencode_sse::tests`; the live round-trip is verified by the
//! orchestrator against a real opencode server.

use super::opencode_sse::{
    parse_event, ChatPartEvent, ChatPartUpdate, MessageMeta, ParsedEvent, PromptKind, ResponseRoute,
};
use super::*;
use crate::multiplexer::types::PendingPrompt;
use serde_json::Value;

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
fn endpoint_and_active_round_trip_per_tab() {
    let tab = "test-tab-sp-roundtrip";
    register_endpoint(tab, "http://127.0.0.1:55555".into(), "/work".into());
    assert!(pending_for_tab(tab).is_none(), "no prompt yet");

    ACTIVE.lock().insert(
        tab.to_string(),
        Active {
            prompt: dummy_prompt("Pick one"),
            route: ResponseRoute {
                session_id: "ses_1".into(),
                request_id: "que_1".into(),
                kind: PromptKind::Question,
                option_labels: vec!["A".into(), "B".into()],
            },
        },
    );
    assert_eq!(pending_for_tab(tab).unwrap().question, "Pick one");
    assert_eq!(route_for_tab(tab).unwrap().request_id, "que_1");

    clear_tab(tab);
    assert!(pending_for_tab(tab).is_none());
    assert!(route_for_tab(tab).is_none());
    assert!(ENDPOINTS.lock().get(tab).is_none());
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
