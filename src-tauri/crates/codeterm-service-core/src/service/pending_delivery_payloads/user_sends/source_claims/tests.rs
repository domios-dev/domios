use super::*;
use crate::service::chat_send_transaction::{
    ChatSendLedger, MatchedRow, SourceRecord, SourceRowRef,
};

#[test]
fn source_claim_round_trips_through_durable_index_and_restored_ledger() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let ledger = ChatSendLedger::default();
    let send = ledger.try_start("tab", "payload", None).unwrap();
    ledger
        .attach_client_token(&send.send_id, "client-token".into())
        .unwrap();
    let source = SourceRowRef {
        source_session_id: "session".into(),
        record: SourceRecord::Jsonl {
            incarnation: "incarnation-1".into(),
            record_start: 8192,
            record_row: 3,
        },
    };
    let status = ledger
        .confirm_send_with_row(&send.send_id, Some(&source))
        .unwrap();
    let claim = SendRowClaim::from_status(&status).unwrap();
    store
        .record_source_claims("tab", std::slice::from_ref(&claim))
        .unwrap();
    store
        .record_source_claims("tab", std::slice::from_ref(&claim))
        .unwrap();
    drop(store);
    let reopened = UserSendStore::open(dir.path()).unwrap();
    let claims = reopened.source_claims("tab").unwrap();
    assert_eq!(claims, [claim.clone()]);
    let restored = ChatSendLedger::default();
    restored.restore_source_claim(&claims[0]);
    let status = restored.status(&send.send_id).unwrap();
    assert_eq!(status.correlation_id.as_deref(), Some("client-token"));
    assert_eq!(status.matched_row, Some(MatchedRow { source, ordinal: 1 }));
    assert!(restored.source_row_claimed(&claim.matched_row.source));

    let duplicate = SendRowClaim {
        send_id: uuid::Uuid::new_v4().to_string(),
        ..claim.clone()
    };
    assert!(reopened.record_source_claims("tab", &[duplicate]).is_err());
    assert!(reopened
        .record_source_claims("other-tab", std::slice::from_ref(&claim))
        .is_err());
    assert_eq!(reopened.source_claims("tab").unwrap(), claims);
}

#[test]
fn old_jsonl_claim_without_incarnation_loads_receipts_and_discards_only_the_claim() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let tab = store.tab("tab");
    owner_directory(&tab.dir).unwrap();
    let send_id = uuid::Uuid::new_v4().to_string();
    let receipt = continuity::UserSendReceipt {
        send_id: send_id.clone(),
        tab_id: "tab".into(),
        client_token: Some("token".into()),
        ledger_sequence: 1,
        state: continuity::ReceiptState::Delivered,
    };
    let mut index = serde_json::to_value(UserIndex::default()).unwrap();
    index["receipts"] = serde_json::json!([receipt]);
    index["source_claims"] = serde_json::json!([{
        "send_id": send_id, "tab_id": "tab", "client_token": "token", "ledger_sequence": 1,
        "matched_row": { "ordinal": 1, "source": {
            "source_session_id": "own",
            "record": { "kind": "jsonl", "record_start": 0, "record_row": 0 }
        }}
    }]);
    let valid = SendRowClaim {
        send_id: uuid::Uuid::new_v4().to_string(),
        tab_id: "tab".into(),
        client_token: None,
        ledger_sequence: 2,
        matched_row: MatchedRow {
            source: SourceRowRef::structured("own", "item").unwrap(),
            ordinal: 1,
        },
    };
    index["source_claims"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::to_value(&valid).unwrap());
    let decoded: UserIndex = serde_json::from_value(index.clone()).unwrap();
    assert_eq!(decoded.source_claims.len(), 2);
    assert!(!decoded.source_claims[0].matched_row.source.valid());
    std::fs::write(
        tab.dir.join("index.json"),
        serde_json::to_vec(&index).unwrap(),
    )
    .unwrap();
    drop(store);
    let reopened = UserSendStore::open(dir.path()).unwrap();
    assert_eq!(reopened.source_claims("tab").unwrap(), [valid]);
    let receipts = reopened.receipts("tab").unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].send_id, send_id);
    assert_eq!(receipts[0].client_token.as_deref(), Some("token"));
    assert_eq!(receipts[0].state, continuity::ReceiptState::Delivered);
}

#[test]
fn durable_index_keeps_all_ordinals_for_a_shared_row() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let source = SourceRowRef::structured("thread", "item").unwrap();
    let claims: Vec<_> = (1..=3)
        .map(|ordinal| SendRowClaim {
            send_id: uuid::Uuid::new_v4().to_string(),
            tab_id: "tab".into(),
            client_token: Some(format!("token-{ordinal}")),
            ledger_sequence: ordinal as u64,
            matched_row: MatchedRow {
                source: source.clone(),
                ordinal,
            },
        })
        .collect();
    store.record_source_claims("tab", &claims).unwrap();
    let reopened = UserSendStore::open(dir.path()).unwrap();
    assert_eq!(reopened.source_claims("tab").unwrap(), claims);
}
