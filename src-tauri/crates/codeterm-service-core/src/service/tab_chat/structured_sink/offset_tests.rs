use super::*;
use crate::agent::chat_types::RichMessage;
use crate::multiplexer::state::MultiplexerState;
use std::time::Duration;

#[test]
fn invisible_records_emit_their_consumed_range() {
    let (mux, tab) = MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let (sent, received) = std::sync::mpsc::channel();
    let listener = fixture
        .ui
        .listen(&format!("tab-chat-msg-{tab}"), move |payload| {
            sent.send(payload.clone()).unwrap();
        });
    let sink = make_flatten_sink(
        fixture.service.lifecycle_journal.clone(),
        &fixture.service,
        fixture.service.mux.clone(),
        0,
        Some("session".into()),
        None,
        None,
    );
    sink.emit(
        &tab,
        vec![RichMessage {
            role: "assistant".into(),
            ..Default::default()
        }],
        100,
        200,
    );
    let event = received.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(event["messages"], serde_json::json!([]));
    assert_eq!(event["start_offset"], 100);
    assert_eq!(event["offset"], 200);
    fixture.ui.unlisten(listener);
}

#[test]
fn source_read_error_without_progress_emits_an_unranged_visible_message() {
    let (mux, tab) = MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let (sent, received) = std::sync::mpsc::channel();
    let listener = fixture
        .ui
        .listen(&format!("tab-chat-msg-{tab}"), move |payload| {
            sent.send(payload.clone()).unwrap();
        });
    let sink = make_flatten_sink(
        fixture.service.lifecycle_journal.clone(),
        &fixture.service,
        fixture.service.mux.clone(),
        0,
        Some("session".into()),
        None,
        None,
    );
    sink.emit(
        &tab,
        vec![RichMessage {
            role: "system".into(),
            record_start: Some(100),
            blocks: vec![crate::agent::chat_types::MessageBlock {
                kind: "text".into(),
                data: serde_json::json!({"text": "opaque source failure", "subtype": "transcript_error"}),
            }],
            ..Default::default()
        }],
        100,
        100,
    );
    let event = received.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(event["start_offset"], serde_json::Value::Null);
    assert_eq!(event["offset"], serde_json::Value::Null);
    assert_eq!(event["messages"][0]["type"], "system");
    assert_eq!(event["messages"][0]["content"], "opaque source failure");
    fixture.ui.unlisten(listener);
}
