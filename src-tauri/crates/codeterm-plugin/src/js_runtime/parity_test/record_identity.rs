//! Record anchoring of `parseSessionDelta` rows: each JS provider names the
//! chunk line a row came from and the host maps it to the record's start byte.
//! No native provider implements `parse_session_delta`, so JS is the only path.

use super::*;
use crate::jsonl_window::{anchor_rows, rows_from_plugin};

fn line_start(lines: &[&str], line: usize) -> u64 {
    lines[..line].iter().map(|l| l.len() as u64 + 1).sum()
}

fn anchors(provider: &JsProvider, lines: &[&str], from_offset: u64) -> Vec<(Option<u64>, u32, Option<String>)> {
    let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), body).unwrap();
    provider
        .parse_session_delta(tmp.path(), from_offset)
        .messages
        .into_iter()
        .map(|m| (m.record_start, m.record_row, m.uuid))
        .collect()
}

#[test]
fn tab_chat_identity_claude_rows_anchor_to_their_record_start() {
    if require_plugins().is_none() {
        return;
    }
    let claude = load_js("claude");
    let lines = [
        r#"{"type":"user","uuid":"c-uuid","message":{"role":"user","content":"with uuid é ✓"}}"#,
        r#"{"type":"user","message":{"role":"user","content":"no uuid"}}"#,
        r#"{"type":"not-a-session-row"}"#,
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"answer"}]}}"#,
    ];
    let expected = vec![
        (Some(0), 0, Some("c-uuid".to_string())),
        (Some(line_start(&lines, 1)), 0, None),
        (Some(line_start(&lines, 3)), 0, None),
    ];
    assert_eq!(anchors(&claude, &lines, 0), expected, "multi-record chunk from byte 0");
    assert_eq!(
        anchors(&claude, &lines, line_start(&lines, 1)),
        expected[1..].to_vec(),
        "a record-aligned later read keeps absolute starts"
    );
    assert_eq!(
        anchors(&claude, &lines, 5),
        expected[1..].to_vec(),
        "a read starting mid-record drops the partial head and keeps absolute starts"
    );
}

#[test]
fn tab_chat_identity_codex_rows_anchor_to_their_record_start() {
    if require_plugins().is_none() {
        return;
    }
    let codex = load_js("codex");
    let lines = [
        r#"{"type":"session_meta","payload":{"id":"11111111-1111-1111-1111-111111111111","cwd":"/work/proj"}}"#,
        r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hello codex"}]},"timestamp":"2026-05-01T10:00:00Z"}"#,
        r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"hi there"}]}}"#,
        r#"{"type":"turn_context","payload":{"model":"gpt-5-codex"}}"#,
        r#"{"type":"response_item","payload":{"type":"function_call","name":"shell","arguments":"{\"cmd\":\"ls\"}"}}"#,
    ];
    let expected = vec![
        (Some(line_start(&lines, 1)), 0, None),
        (Some(line_start(&lines, 2)), 0, None),
        (Some(line_start(&lines, 4)), 0, None),
    ];
    assert_eq!(anchors(&codex, &lines, 0), expected, "multi-record chunk from byte 0");
    assert_eq!(
        anchors(&codex, &lines, line_start(&lines, 2)),
        expected[1..].to_vec(),
        "a record-aligned later read keeps absolute starts"
    );
}

#[test]
fn tab_chat_identity_host_numbers_uuidless_rows_within_one_record() {
    let bytes = b"{\"a\":1}\n\n{\"b\":2}\n";
    let values = vec![
        serde_json::json!({ "role": "assistant", "recordIndex": 0 }),
        serde_json::json!({ "role": "assistant", "recordIndex": 2, "uuid": "u-2" }),
        serde_json::json!({ "role": "assistant", "recordIndex": 2 }),
        serde_json::json!({ "role": "assistant", "recordIndex": 2 }),
        serde_json::json!({ "role": "user" }),
        serde_json::json!({ "role": "user", "recordIndex": 3 }),
    ];
    let (mut rows, indexes) = rows_from_plugin(values).expect("well-formed rows");
    anchor_rows(&mut rows, &indexes, bytes, 100);
    let anchored: Vec<(Option<u64>, u32)> =
        rows.iter().map(|r| (r.record_start, r.record_row)).collect();
    assert_eq!(
        anchored,
        vec![
            (Some(100), 0),
            (Some(109), 0),
            (Some(109), 0),
            (Some(109), 1),
            (None, 0),
            (None, 0),
        ],
        "uuid rows are not numbered; missing or past-end indexes stay unanchored"
    );
    assert!(
        serde_json::to_value(&rows[0]).unwrap().get("recordIndex").is_none(),
        "the host anchor never reaches the wire"
    );
    assert!(
        rows_from_plugin(vec![serde_json::json!({ "role": "user", "blocks": 7 })]).is_none(),
        "one malformed row still fails the chunk"
    );
}
