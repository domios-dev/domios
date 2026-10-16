use super::*;

fn row(session: &str, start: u64) -> SourceRowRef {
    SourceRowRef::jsonl(
        session,
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
