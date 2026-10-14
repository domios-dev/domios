use super::*;
use crate::service::chat_send_transaction::SendLiveBinding;
use crate::service::pending_delivery_payloads::user_sends::continuity::{
    ReceiptState, UserSendReceipt,
};

fn written(ledger: &ChatSendLedger) -> ChatSendStatus {
    let send = ledger.try_start("tab", "opaque-turn", None).unwrap();
    assert!(ledger.bind_rpc_send(
        &send.send_id,
        SendLiveBinding {
            session_id: Some("session".into()),
            binding_nonce: Some("launch".into()),
        },
    ));
    ledger.mark_rpc_accepted(&send.send_id).unwrap()
}

#[test]
fn missing_completion_expires_at_injected_deadline_and_keeps_late_exact_evidence() {
    let ledger = ChatSendLedger::default();
    let send = written(&ledger);
    let now = Instant::now();
    let receiver = arm_at(&mut ledger.state.lock(), &send.send_id, now);
    assert!(expire_at(
        &mut ledger.state.lock(),
        &send.send_id,
        now + EVIDENCE_WINDOW - Duration::from_nanos(1)
    )
    .is_none());
    let expired = expire_at(
        &mut ledger.state.lock(),
        &send.send_id,
        now + EVIDENCE_WINDOW,
    )
    .unwrap();
    assert_eq!(expired.state, ChatSendState::Unknown);
    assert_eq!(expired.error, None);
    assert_eq!(receiver.try_recv(), Err(mpsc::TryRecvError::Disconnected));
    assert!(ledger
        .confirm_rpc_send("tab", "foreign", "launch", &send.send_id)
        .is_none());
    assert!(ledger
        .confirm_rpc_send("tab", "session", "new-launch", &send.send_id)
        .is_none());
    assert_eq!(
        ledger
            .confirm_rpc_send("tab", "session", "launch", &send.send_id)
            .unwrap()
            .state,
        ChatSendState::Sent
    );
    assert_eq!(
        ledger
            .finish(
                &send.send_id,
                Err("submit_unconfirmed: late completion".into())
            )
            .unwrap()
            .state,
        ChatSendState::Sent
    );
}

#[test]
fn exact_evidence_cancels_deadline_without_downgrading_delivery() {
    let ledger = ChatSendLedger::default();
    let send = written(&ledger);
    let now = Instant::now();
    let receiver = arm_at(&mut ledger.state.lock(), &send.send_id, now);
    ledger
        .confirm_rpc_send("tab", "session", "launch", &send.send_id)
        .unwrap();
    assert_eq!(receiver.try_recv(), Err(mpsc::TryRecvError::Disconnected));
    assert!(expire_at(
        &mut ledger.state.lock(),
        &send.send_id,
        now + EVIDENCE_WINDOW
    )
    .is_none());
    assert_eq!(
        ledger.status(&send.send_id).unwrap().state,
        ChatSendState::Sent
    );
}

#[test]
fn expired_receipt_round_trip_never_restores_sending_or_retry_and_repairs_live_written() {
    let ledger = ChatSendLedger::default();
    let send = written(&ledger);
    let receipt = UserSendReceipt {
        send_id: send.send_id.clone(),
        tab_id: send.tab_id.clone(),
        client_token: Some("token".into()),
        ledger_sequence: send.sequence,
        state: ReceiptState::Uncertain,
    };
    let persisted = serde_json::to_string(&receipt).unwrap();
    let restored: UserSendReceipt = serde_json::from_str(&persisted).unwrap();
    ledger.restore_user_receipt(&restored);
    let reopened = ChatSendLedger::default();
    reopened.restore_user_receipt(&restored);
    for current in [&ledger, &reopened] {
        let status = current.reconcile("tab").remove(0);
        assert_eq!(status.state, ChatSendState::Unknown);
        assert_eq!(status.error, None);
        assert_eq!(status.send_id, send.send_id);
    }
    ledger
        .confirm_rpc_send("tab", "session", "launch", &send.send_id)
        .unwrap();
    ledger.restore_user_receipt(&restored);
    assert_eq!(
        ledger.status(&send.send_id).unwrap().state,
        ChatSendState::Sent
    );
}

#[test]
fn late_transport_error_after_expiry_cannot_offer_retry() {
    let ledger = ChatSendLedger::default();
    let send = written(&ledger);
    let now = Instant::now();
    let _receiver = arm_at(&mut ledger.state.lock(), &send.send_id, now);
    expire_at(
        &mut ledger.state.lock(),
        &send.send_id,
        now + EVIDENCE_WINDOW,
    )
    .unwrap();
    assert_eq!(
        ledger
            .finish(&send.send_id, Err("transport closed".into()))
            .unwrap()
            .state,
        ChatSendState::Unknown
    );
}

#[test]
fn pty_deadline_survives_rebind_as_uncertainty_and_never_reopens_written_input() {
    use crate::service::chat_send_transaction::{EvidenceWindow, SendEvidenceAnchor};
    use crate::service::pending_delivery_payloads::user_sends::{UserSendAuthority, UserSendStore};
    let ledger = ChatSendLedger::default();
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let authority = UserSendAuthority {
        tab_id: "tab".into(),
        pty_id: Some("pty".into()),
        binding_nonce: "launch".into(),
        pty_session_generation: Some(1),
        provider_id: "fixture".into(),
        session_id: Some("session".into()),
        observed_session_id: Some("session".into()),
        prelaunch: false,
    };
    let reservation = store
        .reserve(authority.clone(), Some("token".into()), || {
            ledger.reserve_identity()
        })
        .unwrap();
    let record = store
        .admit(&reservation, "opaque-turn", "opaque-turn", None)
        .unwrap();
    drop(reservation);
    let send = ledger.admit_user(&record);
    assert!(store.claim(&record, "session").unwrap());
    assert!(ledger.begin_user_handoff(
        &record,
        "opaque-turn",
        crate::service::chat_send_evidence::CapturedSendEvidence {
            anchor: Some(SendEvidenceAnchor {
                provider_id: "fixture".into(),
                cwd: "/opaque".into(),
                session_id: "session".into(),
                window: EvidenceWindow::Jsonl {
                    transcript_offset: 100
                },
                turn_normalization: Default::default()
            }),
            live_binding: Some(SendLiveBinding {
                session_id: Some("session".into()),
                binding_nonce: Some("launch".into())
            }),
            ..Default::default()
        }
    ));
    ledger.mark_user_written(&send.send_id).unwrap();
    let now = Instant::now();
    let _receiver = arm_at(&mut ledger.state.lock(), &send.send_id, now);
    expire_at(
        &mut ledger.state.lock(),
        &send.send_id,
        now + EVIDENCE_WINDOW,
    )
    .unwrap();
    assert!(ledger.mark_user_written(&send.send_id).is_none());
    let mut rebound = authority.clone();
    rebound.binding_nonce = "next-launch".into();
    assert!(!ledger.rebind_queued_user_authority(&send.send_id, &authority, &rebound));
    store.finish_handoff(&record, false).unwrap();
    store.complete(&record).unwrap();
    drop(store);
    let reopened_store = UserSendStore::open(dir.path()).unwrap();
    assert!(reopened_store.queued("tab").unwrap().is_empty());
    let restarted_ledger = ChatSendLedger::default();
    for receipt in reopened_store.receipts("tab").unwrap() {
        restarted_ledger.restore_user_receipt(&receipt);
    }
    assert_eq!(
        restarted_ledger.status(&send.send_id).unwrap().state,
        ChatSendState::Unknown
    );
    assert_eq!(
        restarted_ledger
            .status(&send.send_id)
            .unwrap()
            .correlation_id
            .as_deref(),
        Some("token")
    );
    assert!(ledger.user_payload_may_have_been_written(&send.send_id));
    assert!(ledger
        .evidence_candidates("tab")
        .iter()
        .any(|candidate| candidate.send_id == send.send_id));
}
