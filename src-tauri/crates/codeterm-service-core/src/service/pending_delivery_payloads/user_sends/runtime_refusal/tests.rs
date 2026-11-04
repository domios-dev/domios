use super::*;
use crate::service::chat_send_transaction::{ChatSendLedger, ChatSendState, SendLiveBinding};

#[test]
fn rpc_refusal_is_durable_before_retry_and_later_evidence_discards_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let ledger = ChatSendLedger::default();
    let authority = UserSendAuthority {
        tab_id: "tab".into(),
        pty_id: None,
        binding_nonce: "nonce".into(),
        pty_session_generation: None,
        provider_id: "fixture".into(),
        session_id: Some("thread".into()),
        observed_session_id: Some("thread".into()),
        prelaunch: false,
    };
    let reservation = store
        .reserve(authority, Some("token".into()), || {
            ledger.reserve_identity()
        })
        .unwrap();
    let record = store.admit(&reservation, "opaque", "opaque", None).unwrap();
    drop(reservation);
    ledger.admit_user(&record);
    assert!(ledger.bind_rpc_send(
        &record.send_id,
        SendLiveBinding {
            session_id: Some("thread".into()),
            binding_nonce: Some("nonce".into()),
        }
    ));
    assert!(store.claim(&record, Some("thread")).unwrap());
    ledger.mark_rpc_accepted(&record.send_id).unwrap();
    let candidate = ledger
        .rpc_refusal_candidate("tab", "thread", "nonce", &record.send_id)
        .unwrap();
    assert!(!store
        .persist_rpc_refusal(&candidate, "foreign", "nonce")
        .unwrap());
    assert!(!store
        .persist_rpc_refusal(&candidate, "thread", "foreign")
        .unwrap());
    assert!(store
        .persist_rpc_refusal(&candidate, "thread", "nonce")
        .unwrap());
    assert_eq!(
        ledger.status(&record.send_id).unwrap().state,
        ChatSendState::Written
    );
    let reopened = UserSendStore::open(dir.path()).unwrap();
    let receipts = reopened.receipts("tab").unwrap();
    assert_eq!(receipts[0].state, ReceiptState::RpcRefused);
    let restored = ChatSendLedger::default();
    restored.restore_user_receipt(&receipts[0]);
    assert_eq!(
        restored.status(&record.send_id).unwrap().state,
        ChatSendState::Failed
    );
    ledger
        .refuse_rpc_send("tab", "thread", "nonce", &record.send_id)
        .unwrap();
    // The owner may finish its write classification after the error; that cannot undo refusal.
    store.finish_handoff(&record, false).unwrap();
    store.complete(&record).unwrap();
    assert_eq!(
        store.receipts("tab").unwrap()[0].state,
        ReceiptState::RpcRefused
    );
    let delivered = ledger
        .confirm_rpc_send("tab", "thread", "nonce", &record.send_id, None)
        .unwrap();
    store.persist_runtime_status(&delivered).unwrap();
    assert_eq!(
        UserSendStore::open(dir.path())
            .unwrap()
            .receipts("tab")
            .unwrap()[0]
            .state,
        ReceiptState::Delivered
    );
    assert!(!store
        .persist_rpc_refusal(&candidate, "thread", "nonce")
        .unwrap());
    restored.restore_user_receipt(&store.receipts("tab").unwrap()[0]);
    assert_eq!(
        restored.status(&record.send_id).unwrap().state,
        ChatSendState::Sent
    );
}
