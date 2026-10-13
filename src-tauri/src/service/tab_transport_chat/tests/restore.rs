use super::InstructionRecordingBackend;
use crate::agent::chat_types::ChatMsg;
use crate::service::tab_chat::chatbackend_history::{read_messages, ChatBackendHistory};
use parking_lot::Mutex;
use serde_json::json;
use std::sync::Arc;

#[test]
fn restored_session_context_roundtrips_persisted_rows_before_first_send() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("own-session.jsonl");
    let rows = vec![
        ChatMsg {
            id: "user-7".into(),
            kind: "user".into(),
            content: "marker-before".into(),
            from_tab: Some("sender-tab".into()),
            ..Default::default()
        },
        ChatMsg {
            id: "assistant-8".into(),
            kind: "assistant".into(),
            content: "reply-before".into(),
            ..Default::default()
        },
    ];
    ChatBackendHistory::open_at(path.clone()).upsert(&rows);
    let restore = super::super::read_chatbackend_restore_context(&path, "own-session").unwrap();
    let opens = Arc::new(Mutex::new(Vec::new()));
    let backend = InstructionRecordingBackend {
        session_config: true,
        open_contexts: opens.clone(),
    };
    let mut ctx = super::super::build_chatbackend_open_session_ctx(
        "own-tab",
        None,
        None,
        json!({"systemPrompt": "instructions"}),
    );
    ctx["restore"] = restore;
    super::super::open_chatbackend_session(&backend, ctx).unwrap();
    let contexts = opens.lock();
    assert_eq!(contexts.len(), 1);
    assert_eq!(contexts[0]["restore"]["sessionId"], "own-session");
    assert_eq!(
        contexts[0]["restore"]["messages"],
        serde_json::to_value(&rows).unwrap()
    );
    assert_eq!(contexts[0]["restore"]["messages"][0]["type"], "user");
    assert_eq!(
        contexts[0]["restore"]["messages"][0]["fromTab"],
        "sender-tab"
    );
    assert_eq!(read_messages(&path), rows);
}

#[test]
fn unreadable_restore_history_refuses_instead_of_opening_empty_context() {
    let dir = tempfile::tempdir().unwrap();
    let error =
        super::super::read_chatbackend_restore_context(dir.path(), "own-session").unwrap_err();
    assert!(error.contains("history unavailable"));
}

#[test]
fn missing_restore_history_is_an_explicit_empty_session() {
    let dir = tempfile::tempdir().unwrap();
    let restore = super::super::read_chatbackend_restore_context(
        &dir.path().join("missing.jsonl"),
        "own-session",
    )
    .unwrap();
    assert_eq!(restore, json!({"sessionId": "own-session", "messages": []}));
}
