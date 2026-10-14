use super::*;

#[test]
fn glued_consumed_owner_record_is_evidence_only_after_its_send_anchor() {
    let records = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../plugins/provider-claude/tests/fixtures/owner-mid-turn-2.1.296.jsonl"
    ));
    let record = records
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|record| {
            record["attachment"]["delivery_id"] == "5ec30b98-8ae2-4e33-ac39-868fadfdae67"
        })
        .unwrap();
    let prompt = record["attachment"]["prompt"].as_str().unwrap();
    let payload = prompt.split_once("<domios").unwrap().0;
    let fx = Fixture::new();
    fx.append_user_turn("own", prompt);
    let anchor = fx.anchor_now("own");
    let send = fx
        .ledger
        .try_start("tab", payload, Some(anchor.clone()))
        .unwrap();
    fx.ledger.finish(&send.send_id, Ok(())).unwrap();
    fx.append_user_turn("foreign", prompt);
    fx.append_user_turn("own", &payload[..payload.len() - 2]);
    scan_transcript_evidence(&fx.registry, &fx.ledger, "tab", |_| {
        panic!("pre-send/foreign/partial evidence")
    });
    fx.append_user_turn("own", prompt);
    let mut confirmed = Vec::new();
    scan_transcript_evidence(&fx.registry, &fx.ledger, "tab", |status| {
        confirmed.push(status)
    });
    assert_eq!(confirmed.len(), 1);
    assert_eq!(confirmed[0].send_id, send.send_id);
    assert_eq!(confirmed[0].state, ChatSendState::Sent);
    assert!(contains_exact_user_turn_since(
        &fx.registry,
        &anchor,
        payload,
        &mut ExactTurnCursor::for_anchor(Some(&anchor))
    ));
}

#[test]
fn tool_result_evidence_preserves_full_payload_without_joining_partial_blocks() {
    let rich = RichMessage {
        role: "assistant".into(),
        blocks: vec![MessageBlock {
            kind: "tool_result".into(),
            data: serde_json::json!({"content": [
                {"type": "text", "text": "prefix\nopaque\nwhole\nsuffix"},
                {"type": "text", "text": "partial"},
                {"type": "text", "text": "other"}
            ]}),
        }],
        ..Default::default()
    };
    let segments = turn_window::rich_evidence_segments(&rich);
    assert!(segments
        .iter()
        .any(|text| contains_written_payload(text, "opaque\nwhole")));
    assert!(!segments
        .iter()
        .any(|text| contains_written_payload(text, "partial\nother")));
    assert!(!segments
        .iter()
        .any(|text| contains_written_payload(text, "opaque\nwhole\nmissing")));
}
