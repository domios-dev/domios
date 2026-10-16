use super::*;
use crate::service::chat_send_transaction::{MatchedRow, SourceRecord, SourceRowRef};

#[test]
fn closed_chat_scan_returns_the_exact_matched_jsonl_record_ref() {
    let fx = Fixture::new();
    fx.append_user_turn("own", "prior");
    let anchor = fx.anchor_now("own");
    let EvidenceWindow::Jsonl { transcript_offset } = anchor.window else {
        panic!("JSONL fixture");
    };
    let send = fx.ledger.try_start("tab", "payload", Some(anchor)).unwrap();
    fx.ledger.finish(&send.send_id, Ok(())).unwrap();
    fx.append_user_turn("foreign", "payload");
    assert!(fx.scan("tab").is_empty());
    fx.append_user_turn("own", "payload");
    let confirmed = fx.scan("tab");
    let incarnation = crate::service::tab_chat::resume::capture_incarnation(
        &fx.source,
        &crate::service::tab_chat::resume_cursor::ChatEpoch {
            source_session_id: "own".into(),
            session_binding_nonce: None,
        },
        &fx.session_path("own"),
        transcript_offset,
        None,
    )
    .unwrap();
    assert_eq!(confirmed.len(), 1);
    assert_eq!(confirmed[0].send_id, send.send_id);
    assert_eq!(
        confirmed[0].matched_row,
        Some(MatchedRow {
            source: SourceRowRef {
                source_session_id: "own".into(),
                record: SourceRecord::Jsonl {
                    incarnation: incarnation.id.to_string(),
                    record_start: transcript_offset,
                    record_row: 0,
                }
            },
            ordinal: 1,
        })
    );
    assert!(fx.scan("tab").is_empty());
}

#[test]
fn closed_chat_scan_distinguishes_recreated_rows_at_the_same_offset() {
    let fx = Fixture::new();
    let first = fx
        .ledger
        .try_start("tab", "repeat", Some(fx.anchor_now("own")))
        .unwrap();
    fx.ledger.finish(&first.send_id, Ok(())).unwrap();
    fx.append_user_turn("own", "repeat");
    let first_status = fx.scan("tab").pop().unwrap();
    std::fs::remove_file(fx.session_path("own")).unwrap();
    assert!(matches!(
        fx.source.capture("own", &fx.session_path("own")),
        SourceCapture::Absent { .. }
    ));
    let second = fx
        .ledger
        .try_start("tab", "repeat", Some(fx.anchor_now("own")))
        .unwrap();
    fx.ledger.finish(&second.send_id, Ok(())).unwrap();
    fx.append_user_turn("own", "repeat");
    let confirmed = fx.scan("tab");
    assert_eq!(confirmed.len(), 1);
    assert_eq!(confirmed[0].send_id, second.send_id);
    let first_row = first_status.matched_row.unwrap();
    let second_row = confirmed[0].matched_row.as_ref().unwrap();
    match (&first_row.source.record, &second_row.source.record) {
        (
            SourceRecord::Jsonl {
                incarnation: first,
                record_start: first_start,
                record_row: first_row,
            },
            SourceRecord::Jsonl {
                incarnation: second,
                record_start: second_start,
                record_row: second_row,
            },
        ) => {
            assert_ne!(first, second);
            assert_eq!((*first_start, *first_row), (0, 0));
            assert_eq!((*second_start, *second_row), (0, 0));
        }
        _ => panic!("JSONL fixture"),
    }
    assert!(fx.scan("tab").is_empty());
}

#[test]
fn jsonl_record_text_matching_keeps_its_reference() {
    let rich = RichMessage {
        role: "user".into(),
        record_start: Some(1234),
        record_row: 5,
        blocks: vec![MessageBlock {
            kind: "text".into(),
            data: serde_json::json!({"text":"payload"}),
        }],
        ..Default::default()
    };
    let turns = turn_window::jsonl_evidence_turns("own", Some("incarnation-1"), &rich);
    assert_eq!(turns[0].content, "payload");
    assert_eq!(
        turns[0].source_row.as_ref().unwrap().record,
        SourceRecord::Jsonl {
            incarnation: "incarnation-1".into(),
            record_start: 1234,
            record_row: 5
        }
    );
}

#[test]
fn closed_chat_jsonl_merged_row_claims_sends_across_append_boundaries() {
    let fx = Fixture::new();
    fx.append_user_turn("own", "prior");
    let first = fx
        .ledger
        .try_start("tab", "first", Some(fx.anchor_now("own")))
        .unwrap();
    fx.ledger.finish(&first.send_id, Ok(())).unwrap();
    fx.append_user_turn("own", "unrelated");
    let second = fx
        .ledger
        .try_start("tab", "second", Some(fx.anchor_now("own")))
        .unwrap();
    fx.ledger.finish(&second.send_id, Ok(())).unwrap();
    fx.append_user_turn("own", "first second");
    let statuses = fx.scan("tab");
    assert_eq!(statuses.len(), 2);
    assert_eq!(statuses[0].send_id, first.send_id);
    assert_eq!(statuses[1].send_id, second.send_id);
    let first_row = statuses[0].matched_row.as_ref().unwrap();
    let second_row = statuses[1].matched_row.as_ref().unwrap();
    assert_eq!(first_row.source, second_row.source);
    assert_eq!((first_row.ordinal, second_row.ordinal), (1, 2));
}

#[test]
fn closed_chat_structured_merged_row_keeps_thread_and_item() {
    let fx = StructuredFixture::new();
    fx.append_user_row("own", "prior");
    let first = fx
        .ledger
        .try_start("tab", "first", Some(fx.anchor_now("own")))
        .unwrap();
    let second = fx
        .ledger
        .try_start("tab", "second", Some(fx.anchor_now("own")))
        .unwrap();
    fx.ledger.finish(&first.send_id, Ok(())).unwrap();
    fx.ledger.finish(&second.send_id, Ok(())).unwrap();
    fx.append_user_row("own", "first second");
    let statuses = fx.scan("tab");
    assert_eq!(statuses.len(), 2);
    let first_row = statuses[0].matched_row.as_ref().unwrap();
    let second_row = statuses[1].matched_row.as_ref().unwrap();
    assert_eq!(first_row.source, second_row.source);
    assert_eq!((first_row.ordinal, second_row.ordinal), (1, 2));
    assert_eq!(first_row.source.source_session_id, "own");
    assert!(
        matches!(&first_row.source.record, SourceRecord::Structured {thread_id, item_id}
        if thread_id == "own" && !item_id.is_empty())
    );
}
