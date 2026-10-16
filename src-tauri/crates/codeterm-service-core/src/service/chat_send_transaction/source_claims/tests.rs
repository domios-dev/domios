use super::*;

fn row(session: &str, start: u64) -> SourceRowRef {
    SourceRowRef::jsonl(
        session,
        "incarnation-1",
        TurnPosition {
            record_start: Some(start),
            record_row: 2,
        },
    )
    .unwrap()
}

fn start(ledger: &ChatSendLedger, text: &str, token: &str) -> ChatSendStatus {
    let send = ledger
        .try_start_scoped(
            "tab",
            text,
            None,
            Some(SendLiveBinding {
                session_id: Some("own".into()),
                binding_nonce: Some("nonce".into()),
            }),
        )
        .unwrap();
    ledger
        .attach_client_token(&send.send_id, token.into())
        .unwrap();
    ledger.finish(&send.send_id, Ok(())).unwrap()
}

#[test]
fn merged_row_claims_have_distinct_ordinals_and_survive_wire_round_trip() {
    let ledger = ChatSendLedger::default();
    let first = start(&ledger, "repeat", "first");
    let second = start(&ledger, "repeat", "second");
    let source = row("own", 34);
    let confirmed = ledger.confirm_user_turn_with_row(
        "tab",
        "own",
        Some("nonce"),
        "repeat repeat",
        Some(&source),
    );
    assert_eq!(
        confirmed
            .iter()
            .map(|status| &status.send_id)
            .collect::<Vec<_>>(),
        [&first.send_id, &second.send_id]
    );
    for (index, status) in confirmed.iter().enumerate() {
        assert_eq!(
            status.matched_row,
            Some(MatchedRow {
                source: source.clone(),
                ordinal: index as u32 + 1
            })
        );
        assert_eq!(
            ledger.status(&status.send_id).unwrap().matched_row,
            status.matched_row
        );
        let wire = serde_json::to_value(status).unwrap();
        assert_eq!(wire["matched_row"]["source"]["source_session_id"], "own");
        assert_eq!(wire["matched_row"]["source"]["record"]["record_start"], 34);
        assert_eq!(
            serde_json::from_value::<ChatSendStatus>(wire).unwrap(),
            *status
        );
    }
    let third = start(&ledger, "repeat", "third");
    assert!(ledger
        .confirm_user_turn_with_row("tab", "own", Some("nonce"), "repeat", Some(&source))
        .is_empty());
    assert!(ledger.status(&third.send_id).unwrap().matched_row.is_none());
    let next = ledger.confirm_user_turn_with_row(
        "tab",
        "own",
        Some("nonce"),
        "repeat",
        Some(&row("own", 98)),
    );
    assert_eq!(next[0].send_id, third.send_id);
    assert_eq!(next[0].matched_row.as_ref().unwrap().ordinal, 1);
}

#[test]
fn recreated_jsonl_row_can_confirm_a_new_send_after_restoring_the_old_claim() {
    let ledger = ChatSendLedger::default();
    let first = start(&ledger, "repeat", "first");
    let source = row("own", 34);
    let status = ledger
        .confirm_send_with_row(&first.send_id, Some(&source))
        .unwrap();
    let claim = SendRowClaim::from_status(&status).unwrap();
    let restored = ChatSendLedger::default();
    restored.restore_source_claim(
        &serde_json::from_value(serde_json::to_value(claim).unwrap()).unwrap(),
    );
    let second = start(&restored, "repeat", "second");
    assert!(restored
        .confirm_send_with_row(&second.send_id, Some(&source))
        .is_none());
    let recreated = SourceRowRef::jsonl(
        "own",
        "incarnation-2",
        TurnPosition {
            record_start: Some(34),
            record_row: 2,
        },
    )
    .unwrap();
    let confirmed = restored.confirm_user_turn_with_row(
        "tab",
        "own",
        Some("nonce"),
        "repeat",
        Some(&recreated),
    );
    assert_eq!(confirmed.len(), 1);
    assert_eq!(confirmed[0].send_id, second.send_id);
    assert_eq!(
        confirmed[0].matched_row,
        Some(MatchedRow {
            source: recreated.clone(),
            ordinal: 1
        })
    );
    assert!(restored.source_row_claimed(&source));
    assert!(restored.source_row_claimed(&recreated));
    let third = start(&restored, "repeat", "third");
    assert!(restored
        .confirm_send_with_row(&third.send_id, Some(&recreated))
        .is_none());
    assert!(SourceRowRef::jsonl(
        "own",
        "",
        TurnPosition {
            record_start: Some(34),
            record_row: 2
        }
    )
    .is_none());
}

#[test]
fn aggregate_claims_exclude_only_proven_other_jsonl_incarnations() {
    let ledger = ChatSendLedger::default();
    let anchor = SendEvidenceAnchor {
        provider_id: "provider".into(),
        cwd: "/proj".into(),
        session_id: "own".into(),
        window: EvidenceWindow::Jsonl {
            transcript_offset: 17,
        },
        turn_normalization: Default::default(),
    };
    let first = ledger
        .try_start("tab", "repeat", Some(anchor.clone()))
        .unwrap();
    let prior = row("own", 34);
    ledger
        .confirm_send_with_row(&first.send_id, Some(&prior))
        .unwrap();
    assert_eq!(
        ledger.claimed_turn_anchors("tab", "own", "repeat", Some(&prior)),
        [anchor.clone()]
    );
    assert_eq!(
        ledger.claimed_turn_anchors("tab", "own", "repeat", None),
        [anchor.clone()]
    );
    let current = SourceRowRef::jsonl(
        "own",
        "incarnation-2",
        TurnPosition {
            record_start: Some(34),
            record_row: 2,
        },
    )
    .unwrap();
    assert!(ledger
        .claimed_turn_anchors("tab", "own", "repeat", Some(&current))
        .is_empty());
    let unknown = ledger
        .try_start("tab", "repeat", Some(anchor.clone()))
        .unwrap();
    ledger.confirm_send(&unknown.send_id).unwrap();
    assert_eq!(
        ledger.claimed_turn_anchors("tab", "own", "repeat", Some(&current)),
        [anchor]
    );
    assert_eq!(
        ledger
            .claimed_turn_anchors("tab", "own", "repeat", Some(&prior))
            .len(),
        2
    );
}

#[test]
fn rebuilt_source_registry_keeps_restored_claims_exclusive() {
    use crate::service::tab_chat::registry::{JsonlSource, SourceObservation};
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("session.jsonl");
    std::fs::write(&path, b"repeat\n").unwrap();
    let source_row = |source: &JsonlSource| {
        let SourceObservation::Present {
            incarnation: Some(incarnation),
            ..
        } = source.observe("own", &path, None, 0)
        else {
            panic!("source incarnation");
        };
        SourceRowRef::jsonl(
            "own",
            &incarnation.id,
            TurnPosition {
                record_start: Some(0),
                record_row: 0,
            },
        )
        .unwrap()
    };
    let ledger = ChatSendLedger::default();
    let first = start(&ledger, "repeat", "first");
    let original = source_row(&JsonlSource::default());
    let status = ledger
        .confirm_send_with_row(&first.send_id, Some(&original))
        .unwrap();
    let claim = SendRowClaim::from_status(&status).unwrap();
    let restored = ChatSendLedger::default();
    restored.restore_source_claim(
        &serde_json::from_value(serde_json::to_value(claim).unwrap()).unwrap(),
    );
    let second = start(&restored, "repeat", "second");
    let rebuilt = source_row(&JsonlSource::default());
    assert_eq!(original, rebuilt);
    assert!(restored.source_row_claimed(&rebuilt));
    assert!(restored
        .confirm_user_turn_with_row("tab", "own", Some("nonce"), "repeat", Some(&rebuilt),)
        .is_empty());
    assert_eq!(
        restored.status(&second.send_id).unwrap().state,
        ChatSendState::Unknown
    );
}

#[test]
fn jsonl_live_evidence_without_incarnation_cannot_fall_back_to_text() {
    let ledger = ChatSendLedger::default();
    let send = start(&ledger, "repeat", "token");
    let provenance = LiveProvenance {
        source_session_id: "own".into(),
        source_binding_nonce: Some("nonce".into()),
        range: crate::service::tab_chat::tailer::SourceRange {
            start_offset: 0,
            offset: 100,
            incarnation: None,
        },
        positions: vec![TurnPosition {
            record_start: Some(0),
            record_row: 0,
        }],
    };
    assert!(delivered_turn_confirmations(
        &ledger,
        "tab",
        "own",
        Some("nonce"),
        &[crate::agent::chat_types::ChatMsg {
            id: "turn".into(),
            kind: "user".into(),
            content: "repeat".into(),
            ..Default::default()
        }],
        Some(&provenance),
    )
    .is_empty());
    assert_eq!(
        ledger.status(&send.send_id).unwrap().state,
        ChatSendState::Unknown
    );
}

#[test]
fn foreign_session_row_cannot_claim_a_send() {
    let ledger = ChatSendLedger::default();
    let send = start(&ledger, "repeat", "token");
    let foreign = row("foreign", 34);
    assert!(ledger
        .confirm_user_turn_with_row("tab", "own", Some("nonce"), "repeat", Some(&foreign))
        .is_empty());
    assert!(ledger
        .confirm_send_with_row(&send.send_id, Some(&foreign))
        .is_none());
    assert!(ledger.status(&send.send_id).unwrap().matched_row.is_none());
    let valid = row("own", 34);
    assert!(ledger
        .confirm_send_with_row(&send.send_id, Some(&valid))
        .is_some());
}

#[test]
fn structured_row_requires_the_exact_thread_and_item() {
    let ledger = ChatSendLedger::default();
    let send = start(&ledger, "repeat", "token");
    let invalid = SourceRowRef {
        source_session_id: "own".into(),
        record: SourceRecord::Structured {
            thread_id: "foreign".into(),
            item_id: "item".into(),
        },
    };
    assert!(ledger
        .confirm_send_with_row(&send.send_id, Some(&invalid))
        .is_none());
    let source = SourceRowRef::structured("own", "item").unwrap();
    let status = ledger
        .confirm_send_with_row(&send.send_id, Some(&source))
        .unwrap();
    assert_eq!(status.matched_row.unwrap().source, source);
}
