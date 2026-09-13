use super::*;
use crate::agent::chat_types::{MessageBlock, RichMessage};
use crate::agent::provider::ModelInfo;
use crate::chatbackend::{ChatBackendError, ChatBackendProvider};
use serde_json::json;

/// Cumulative-transcript backend (like the lmstudio plugin): `poll` with no
/// cursor returns the FULL transcript, so the snapshot can slice it.
struct SnapshotBackend {
    messages: Vec<ChatMsg>,
}

impl SnapshotBackend {
    fn with_messages(n: usize) -> Self {
        let messages = (0..n)
            .map(|i| ChatMsg {
                id: i.to_string(),
                kind: "assistant".into(),
                content: format!("m{i}"),
                ..Default::default()
            })
            .collect();
        Self { messages }
    }
}

impl ChatBackendProvider for SnapshotBackend {
    fn id(&self) -> &str {
        "snapshot"
    }
    fn open_session(&self, _ctx: serde_json::Value) -> Result<String, ChatBackendError> {
        Ok("sid".into())
    }
    fn send_message(&self, _session_id: &str, _text: &str) -> Result<(), ChatBackendError> {
        Ok(())
    }
    fn poll(
        &self,
        _session_id: &str,
        _cursor: Option<&str>,
    ) -> Result<crate::agent::chat_types::StructuredChat, ChatBackendError> {
        Ok(crate::agent::chat_types::StructuredChat {
            messages: self.messages.clone(),
            cursor: None,
            done: None,
        })
    }
    fn close_session(&self, _session_id: &str) -> Result<(), ChatBackendError> {
        Ok(())
    }
    fn list_models(&self) -> Result<Vec<ModelInfo>, ChatBackendError> {
        Ok(Vec::new())
    }
}

#[test]
fn chatbackend_snapshot_returns_full_transcript() {
    let backend = SnapshotBackend::with_messages(3);
    let (msgs, total) = chatbackend_transcript_snapshot(&backend, "sid", 0).unwrap();
    assert_eq!(total, 3);
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0].content, "m0");
    assert_eq!(msgs[2].content, "m2");
}

#[test]
fn chatbackend_snapshot_honors_since_cursor() {
    let backend = SnapshotBackend::with_messages(3);
    let (msgs, total) = chatbackend_transcript_snapshot(&backend, "sid", 2).unwrap();
    // total is the full count; the slice starts at `since`.
    assert_eq!(total, 3);
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].content, "m2");
}

#[test]
fn chatbackend_snapshot_since_past_end_is_empty_not_error() {
    let backend = SnapshotBackend::with_messages(2);
    let (msgs, total) = chatbackend_transcript_snapshot(&backend, "sid", 5).unwrap();
    assert_eq!(total, 2);
    assert!(msgs.is_empty());
}

#[test]
fn chatbackend_snapshot_reads_hand_written_jsonl() {
    // live `poll`, the persisted file wins (refresh shows real history).
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    std::fs::write(
        &path,
        "{\"id\":\"u1\",\"type\":\"user\",\"content\":\"hi\"}\n\
         {\"id\":\"a1\",\"type\":\"assistant\",\"content\":\"hello there\"}\n",
    )
    .unwrap();

    let backend = SnapshotBackend::with_messages(0); // live Map empty
    let (msgs, total) = super::chatbackend_snapshot_from(&backend, "sid", 0, &path).unwrap();
    assert_eq!(total, 2);
    assert_eq!(msgs[0].content, "hi");
    assert_eq!(msgs[1].content, "hello there");
}

#[test]
fn restored_tab_replays_history_from_jsonl_when_live_map_wiped() {
    // (poll → 0 messages) but the JSONL persisted the full transcript.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    let mut h = chatbackend_history::ChatBackendHistory::open_at(path.clone());
    h.upsert(&[
        ChatMsg {
            id: "1".into(),
            kind: "user".into(),
            content: "remember me".into(),
            ..Default::default()
        },
        ChatMsg {
            id: "2".into(),
            kind: "assistant".into(),
            content: "i do".into(),
            ..Default::default()
        },
    ]);

    let wiped = SnapshotBackend::with_messages(0);
    let (msgs, total) = super::chatbackend_snapshot_from(&wiped, "sid", 0, &path).unwrap();
    assert_eq!(total, 2, "history replays from disk, not the wiped Map");
    assert_eq!(msgs[0].content, "remember me");

    // `since` is honored on the disk-backed slice too.
    let (tail, _) = super::chatbackend_snapshot_from(&wiped, "sid", 1, &path).unwrap();
    assert_eq!(tail.len(), 1);
    assert_eq!(tail[0].content, "i do");
}

#[test]
fn snapshot_after_poll_flush_includes_host_inbound_and_reply() {
    let plugin = "test-snapshot-single-writer-unique";
    let session = "snap-sw-sid";
    let path = chatbackend_history::session_path(plugin, session);
    let _ = std::fs::remove_file(&path);

    let shared = chatbackend_history::ChatBackendHistory::shared(plugin, session);
    shared.lock().upsert(&[ChatMsg {
        id: "inbound-1".into(),
        kind: "user".into(),
        content: "ping from Gauss".into(),
        from_label: Some("Gauss".into()),
        inbound_kind: Some("tab".into()),
        ..Default::default()
    }]);
    // Poll-loop flush of the reply (delta without the inbound row).
    shared.lock().upsert(&[ChatMsg {
        id: "reply-1".into(),
        kind: "assistant".into(),
        content: "pong".into(),
        ..Default::default()
    }]);

    let wiped = SnapshotBackend::with_messages(0); // live Map empty (refresh)
    let (msgs, total) = super::chatbackend_snapshot_from(&wiped, session, 0, &path).unwrap();
    assert_eq!(total, 2, "inbound + reply both survive to the snapshot");
    assert_eq!(msgs[0].content, "ping from Gauss");
    assert_eq!(msgs[0].from_label.as_deref(), Some("Gauss"));
    assert_eq!(msgs[1].content, "pong");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn chatbackend_snapshot_falls_back_to_live_poll_without_jsonl() {
    // disk ⇒ read the live plugin transcript.
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nope.jsonl");
    let backend = SnapshotBackend::with_messages(2);
    let (msgs, total) = super::chatbackend_snapshot_from(&backend, "sid", 0, &missing).unwrap();
    assert_eq!(total, 2);
    assert_eq!(msgs[0].content, "m0");
}

#[test]
fn live_chat_poll_interval_stays_low_latency() {
    assert!(live_chat_poll_interval() <= std::time::Duration::from_millis(150));
}

#[test]
fn rich_text_and_tool_flatten() {
    let rich = RichMessage {
        role: "assistant".into(),
        blocks: vec![
            MessageBlock {
                kind: "text".into(),
                data: json!({"text": "hi"}),
            },
            MessageBlock {
                kind: "tool_use".into(),
                data: json!({"name": "Bash", "input": {"command": "codeterm tab list"}}),
            },
        ],
        ..Default::default()
    };
    let msgs = rich_to_chat_msgs(&rich, 0);
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].kind, "assistant");
    assert_eq!(msgs[0].content, "hi");
    assert_eq!(msgs[1].kind, "tool_call");
    assert_eq!(msgs[1].tool_name.as_deref(), Some("Bash"));
    assert_eq!(
        msgs[1].tool_input.as_ref().unwrap()["command"],
        "codeterm tab list"
    );
}

#[test]
fn canonical_tool_fields_passthrough_without_input_summary() {
    let rich = RichMessage {
        role: "assistant".into(),
        blocks: vec![
            MessageBlock {
                kind: "tool_use".into(),
                data: json!({
                    "name": "provider_specific_name",
                    "input": {"command": "must not become a summary"},
                    "tool_kind": "command",
                    "tool_summary": "Run command",
                    "tool_detail": "cargo check",
                    "tool_edits": [{"path": "src/lib.rs"}]
                }),
            },
            MessageBlock {
                kind: "tool_use".into(),
                data: json!({
                    "name": "another_provider_name",
                    "input": {"command": "must not become JSON"}
                }),
            },
        ],
        ..Default::default()
    };

    let msgs = rich_to_chat_msgs(&rich, 0);
    assert_eq!(msgs[0].tool_kind.as_deref(), Some("command"));
    assert_eq!(msgs[0].tool_summary.as_deref(), Some("Run command"));
    assert_eq!(msgs[0].tool_detail.as_deref(), Some("cargo check"));
    assert_eq!(
        msgs[0].tool_edits.as_ref().unwrap()[0]["path"],
        "src/lib.rs"
    );
    assert_eq!(msgs[0].content, "Run command");
    assert_eq!(msgs[1].tool_summary, None);
    assert_eq!(msgs[1].tool_detail, None);
    assert_eq!(msgs[1].tool_args, None);
    assert_eq!(msgs[1].content, "");
}

#[test]
fn tool_activity_capabilities_and_flow_boundaries_pass_through() {
    let rich = RichMessage {
        role: "assistant".into(),
        blocks: vec![
            MessageBlock {
                kind: "tool_flow".into(),
                data: json!({ "state": "start", "flow_id": "turn-a" }),
            },
            MessageBlock {
                kind: "tool_use".into(),
                data: json!({
                    "name": "Bash",
                    "input": {"command": "npm test", "description": "Run tests"},
                    "tool_kind": "command",
                    "tool_summary": "npm test",
                    "tool_description": "Run tests",
                    "tool_title": "npm test",
                    "tool_flow_id": "msg-1",
                    "tool_echo": true,
                    "tool_rich_result": {"kind": "web", "query": "serde rename", "resultCount": 2}
                }),
            },
            MessageBlock {
                kind: "tool_use".into(),
                data: json!({ "name": "Read", "input": {"file_path": "a.rs"}, "tool_rich_result": "malformed" }),
            },
            MessageBlock {
                kind: "tool_flow".into(),
                data: json!({ "state": "end" }),
            },
            MessageBlock {
                kind: "tool_flow".into(),
                data: json!({ "state": "guess", "flow_id": "turn-b" }),
            },
        ],
        ..Default::default()
    };
    let msgs = rich_to_chat_msgs(&rich, 0);
    assert_eq!(msgs.len(), 4);
    assert_eq!(msgs[0].kind, "tool_flow");
    assert_eq!(msgs[0].content, "start");
    assert_eq!(msgs[0].tool_flow_id.as_deref(), Some("turn-a"));
    assert_eq!(msgs[1].tool_description.as_deref(), Some("Run tests"));
    assert_eq!(msgs[1].tool_title.as_deref(), Some("npm test"));
    assert_eq!(msgs[1].tool_flow_id.as_deref(), Some("msg-1"));
    assert_eq!(msgs[1].tool_input.as_ref().unwrap()["command"], "npm test");
    assert_eq!(msgs[2].tool_description, None);
    assert_eq!(msgs[2].tool_title, None);
    assert_eq!(msgs[2].tool_flow_id, None);
    assert_eq!(msgs[1].tool_echo, Some(true));
    assert_eq!(msgs[2].tool_echo, None);
    assert_eq!(
        msgs[1].tool_rich_result,
        Some(json!({"kind": "web", "query": "serde rename", "resultCount": 2}))
    );
    assert_eq!(msgs[2].tool_rich_result, None);
    assert_eq!(msgs[3].kind, "tool_flow");
    assert_eq!(msgs[3].content, "end");
    assert_eq!(msgs[3].tool_flow_id, None);
}

#[test]
fn file_diff_capability_passes_through_calls_and_results() {
    let diffs = json!([{
        "path": "src/a.rs",
        "additions": 1,
        "deletions": 1,
        "hunks": [{"oldStart": 1, "oldLines": 1, "newStart": 1, "newLines": 1, "lines": ["-a", "+b"]}]
    }]);
    let assistant = RichMessage {
        role: "assistant".into(),
        blocks: vec![
            MessageBlock {
                kind: "tool_use".into(),
                data: json!({"name": "file_change", "id": "fc-1", "tool_kind": "edit", "file_diffs": diffs.clone()}),
            },
            MessageBlock {
                kind: "tool_result".into(),
                data: json!({"tool_use_id": "fc-1", "content": "ok", "file_diffs": diffs.clone()}),
            },
            MessageBlock {
                kind: "tool_use".into(),
                data: json!({"name": "Read", "id": "r-1"}),
            },
        ],
        ..Default::default()
    };
    let msgs = rich_to_chat_msgs(&assistant, 0);
    assert_eq!(msgs[0].file_diffs.as_ref(), Some(&diffs));
    assert_eq!(msgs[1].file_diffs.as_ref(), Some(&diffs));
    assert_eq!(msgs[2].file_diffs, None);

    let user = RichMessage {
        role: "user".into(),
        blocks: vec![MessageBlock {
            kind: "tool_result".into(),
            data: json!({"tool_use_id": "tu-1", "content": "ok", "file_diffs": diffs.clone()}),
        }],
        ..Default::default()
    };
    let msgs = rich_to_chat_msgs(&user, 0);
    assert_eq!(msgs[0].kind, "tool_result");
    assert_eq!(msgs[0].file_diffs.as_ref(), Some(&diffs));
    let wire = serde_json::to_value(&msgs[0]).unwrap();
    assert_eq!(wire["fileDiffs"], diffs);
}

#[test]
fn completed_tool_use_preserves_explicit_lifecycle() {
    let rich = RichMessage {
        role: "assistant".into(),
        blocks: vec![MessageBlock {
            kind: "tool_use".into(),
            data: json!({
                "name": "Read",
                "tool_kind": "read",
                "tool_summary": "src/main.rs",
                "tool_status": "success"
            }),
        }],
        ..Default::default()
    };
    let msgs = rich_to_chat_msgs(&rich, 0);
    assert_eq!(msgs[0].tool_error, Some(false));
    assert_eq!(msgs[0].tool_result, None);
}

#[test]
fn tool_result_propagates_is_error_and_strips_ansi() {
    let rich = RichMessage {
        role: "assistant".into(),
        blocks: vec![MessageBlock {
            kind: "tool_result".into(),
            data: json!({
                "tool_use_id": "tool-1",
                "is_error": true,
                "content": "\u{1b}[31mfailed\u{1b}[0m"
            }),
        }],
        ..Default::default()
    };
    let msgs = rich_to_chat_msgs(&rich, 0);
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].kind, "tool_result");
    assert_eq!(msgs[0].tool_error, Some(true));
    assert_eq!(msgs[0].tool_result.as_deref(), Some("failed"));
}

#[test]
fn user_turn_tool_result_propagates_is_error() {
    let rich = RichMessage {
        role: "user".into(),
        blocks: vec![MessageBlock {
            kind: "tool_result".into(),
            data: json!({
                "tool_use_id": "tool-user-1",
                "is_error": true,
                "content": "boom"
            }),
        }],
        ..Default::default()
    };
    let msgs = rich_to_chat_msgs(&rich, 3);
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].tool_id.as_deref(), Some("tool-user-1"));
    assert_eq!(msgs[0].tool_error, Some(true));
}

#[test]
fn compact_summary_normalizes_to_collapsed_system_item() {
    let rich = RichMessage {
        role: "user".into(),
        blocks: vec![MessageBlock {
            kind: "text".into(),
            data: json!({"text": "This session is being continued from a previous conversation."}),
        }],
        uuid: Some("compact-user".into()),
        is_compact_summary: Some(true),
        is_visible_in_transcript_only: Some(true),
        source_type: Some("user".into()),
        ..Default::default()
    };

    let msgs = rich_to_chat_msgs(&rich, 7);
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].kind, "system");
    assert_eq!(msgs[0].system_subtype.as_deref(), Some("compact_summary"));
    assert!(msgs[0].collapsed);
    assert_eq!(msgs[0].seq, 7);
}

#[test]
fn compact_boundary_is_status_and_noisy_system_rows_are_hidden() {
    let boundary = RichMessage {
        role: "system".into(),
        blocks: vec![MessageBlock {
            kind: "text".into(),
            data: json!({"text": "", "subtype": "compact_boundary"}),
        }],
        uuid: Some("compact-boundary".into()),
        source_type: Some("system".into()),
        source_subtype: Some("compact_boundary".into()),
        ..Default::default()
    };
    let msgs = rich_to_chat_msgs(&boundary, 8);
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].kind, "status");
    assert_eq!(msgs[0].content, "Context compacted");
    assert_eq!(msgs[0].system_subtype.as_deref(), Some("compact_boundary"));

    let turn_duration = RichMessage {
        role: "system".into(),
        blocks: vec![MessageBlock {
            kind: "text".into(),
            data: json!({"text": "123ms", "subtype": "turn_duration"}),
        }],
        source_type: Some("system".into()),
        source_subtype: Some("turn_duration".into()),
        ..Default::default()
    };
    assert!(rich_to_chat_msgs(&turn_duration, 9).is_empty());
}

#[test]
fn provider_status_row_normalizes_to_status() {
    let rich = RichMessage {
        role: "status".into(),
        blocks: vec![MessageBlock {
            kind: "text".into(),
            data: json!({"text": "Task completed", "subtype": "task_notification"}),
        }],
        uuid: Some("task-notification".into()),
        source_type: Some("user".into()),
        source_subtype: Some("task_notification".into()),
        ..Default::default()
    };

    let msgs = rich_to_chat_msgs(&rich, 10);
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].kind, "status");
    assert_eq!(msgs[0].content, "Task completed");
    assert_eq!(msgs[0].system_subtype.as_deref(), Some("task_notification"));
    assert_eq!(msgs[0].seq, 10);
}
