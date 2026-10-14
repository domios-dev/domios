use crate::service::chat_send_transaction::{ChatSendLedger, ChatSendState, SendLiveBinding};
#[test]
fn uncertain_handoff_cannot_replay_after_recovery_and_only_own_live_evidence_delivers() {
    let ledger = ChatSendLedger::default();
    let pending = ledger.try_start("tab", "opaque payload", None).unwrap();
    let bound = SendLiveBinding {
        session_id: Some("thread".into()),
        binding_nonce: Some("nonce".into()),
    };
    assert!(ledger.bind_rpc_send(&pending.send_id, bound.clone()));
    ledger.mark_rpc_uncertain(&pending.send_id);
    assert!(!ledger.bind_rpc_send(&pending.send_id, bound));
    assert!(ledger
        .confirm_rpc_send("other-tab", "thread", "nonce", &pending.send_id)
        .is_none());
    assert!(ledger
        .confirm_rpc_send("tab", "other-thread", "nonce", &pending.send_id)
        .is_none());
    assert!(ledger
        .confirm_rpc_send("tab", "thread", "old-nonce", &pending.send_id)
        .is_none());
    assert!(ledger
        .confirm_rpc_send("tab", "thread", "nonce", &pending.send_id)
        .is_some());
    assert_eq!(
        ledger.status(&pending.send_id).unwrap().state,
        ChatSendState::Sent
    );
    ledger.finish(&pending.send_id, Err("late pipe failure".into()));
    assert_eq!(
        ledger.status(&pending.send_id).unwrap().state,
        ChatSendState::Sent
    );
}
#[test]
fn acceptance_is_written_and_never_fabricates_delivery() {
    let ledger = ChatSendLedger::default();
    let pending = ledger.try_start("tab", "opaque payload", None).unwrap();
    assert!(ledger.bind_rpc_send(
        &pending.send_id,
        SendLiveBinding {
            session_id: Some("thread".into()),
            binding_nonce: Some("nonce".into())
        }
    ));
    ledger.mark_rpc_accepted(&pending.send_id);
    assert_eq!(
        ledger.status(&pending.send_id).unwrap().state,
        ChatSendState::Written
    );
}
