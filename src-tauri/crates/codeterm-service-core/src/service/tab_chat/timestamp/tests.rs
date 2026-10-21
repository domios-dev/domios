use super::*;
use crate::agent::chat_types::{ChatTimelineMarker, MessageBlock};
use crate::service::tab_chat::rich_to_chat_msgs;
use serde_json::json;

#[test]
fn preserves_source_time_without_inventing_missing_time() {
    for (timestamp, ts, expected) in [
        (
            Some("2026-10-11T10:05:00Z"),
            Some(1_791_712_800),
            Some("2026-10-11T10:05:00Z"),
        ),
        (None, Some(1_791_712_800), Some("1791712800")),
        (Some(""), Some(1_791_712_800_000), Some("1791712800000")),
        (None, None, None),
        (None, Some(0), None),
    ] {
        let rich = RichMessage {
            timestamp: timestamp.map(str::to_string),
            ts,
            ..Default::default()
        };
        assert_eq!(rich_message_timestamp(&rich).as_deref(), expected);
    }
}

#[test]
fn numeric_source_time_reaches_every_flattened_block_and_marker() {
    let rich = RichMessage {
        role: "assistant".into(),
        ts: Some(1_791_712_800),
        blocks: vec![
            MessageBlock {
                kind: "text".into(),
                data: json!({"text": "fixture"}),
            },
            MessageBlock {
                kind: "thinking".into(),
                data: json!({"thinking": "fixture"}),
            },
            MessageBlock {
                kind: "tool_use".into(),
                data: json!({"id": "call", "name": "tool", "input": {}}),
            },
            MessageBlock {
                kind: "tool_result".into(),
                data: json!({"tool_use_id": "call", "content": "fixture"}),
            },
            MessageBlock {
                kind: "image".into(),
                data: json!({"source": {"data": "fixture"}}),
            },
        ],
        ..Default::default()
    };
    let rows = rich_to_chat_msgs(&rich, 0);
    assert_eq!(rows.len(), 5);
    assert!(rows
        .iter()
        .all(|row| row.timestamp.as_deref() == Some("1791712800")));
    let marker = RichMessage {
        blocks: vec![],
        timeline_marker: Some(ChatTimelineMarker::ContextCompaction),
        ..rich
    };
    let rows = rich_to_chat_msgs(&marker, 0);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].timestamp.as_deref(), Some("1791712800"));
}
