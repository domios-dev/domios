use super::*;
use crate::service::chat_send_transaction::{ChatSendLedger, ChatSendState};

fn authority() -> UserSendAuthority {
    UserSendAuthority {
        tab_id: "tab".into(),
        pty_id: Some("old-pty".into()),
        binding_nonce: "old-nonce".into(),
        pty_session_generation: Some(1),
        provider_id: "provider".into(),
        session_id: Some("session".into()),
        observed_session_id: Some("session".into()),
        prelaunch: false,
    }
}

fn admit(store: &UserSendStore, sequence: u64) -> UserSendRecord {
    let reservation = store
        .reserve(authority(), Some(format!("token-{sequence}")), || {
            (uuid::Uuid::new_v4().to_string(), sequence)
        })
        .unwrap();
    store
        .admit(&reservation, "payload", "payload", None)
        .unwrap()
}

fn next_launch() -> UserSendAuthority {
    UserSendAuthority {
        pty_id: Some("new-pty".into()),
        binding_nonce: "new-nonce".into(),
        pty_session_generation: Some(2),
        ..authority()
    }
}

#[test]
fn only_never_written_held_sends_follow_the_same_tab_provider_and_known_session() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let record = admit(&store, 1);
    assert!(can_carry_held_send(&record, &next_launch()));
    for current in [
        UserSendAuthority {
            tab_id: "other-tab".into(),
            ..next_launch()
        },
        UserSendAuthority {
            provider_id: "other-provider".into(),
            ..next_launch()
        },
        UserSendAuthority {
            observed_session_id: Some("other-session".into()),
            ..next_launch()
        },
        UserSendAuthority {
            observed_session_id: None,
            ..next_launch()
        },
    ] {
        assert!(!can_carry_held_send(&record, &current));
    }
    for state in [
        UserSendState::Claimed,
        UserSendState::Uncertain,
        UserSendState::BodyLease,
    ] {
        assert!(!can_carry_held_send(
            &UserSendRecord {
                state,
                ..record.clone()
            },
            &next_launch()
        ));
    }
    assert!(can_carry_held_send(
        &UserSendRecord {
            held_before_readiness: false,
            ..record.clone()
        },
        &next_launch()
    ));
    assert!(!can_carry_held_send(
        &UserSendRecord {
            handoff_session: Some("session".into()),
            ..record
        },
        &next_launch()
    ));
}

#[test]
fn only_a_known_different_session_ends_the_session_authority() {
    assert!(!known_session_changed(Some("own"), Some("own")));
    assert!(!known_session_changed(Some("own"), None));
    assert!(!known_session_changed(None, Some("own")));
    assert!(known_session_changed(Some("own"), Some("foreign")));
}

#[test]
fn daemon_restart_carries_originals_in_order_and_claims_once_after_readiness() {
    let dir = tempfile::tempdir().unwrap();
    let old = UserSendStore::open(dir.path()).unwrap();
    let first = admit(&old, 1);
    let second = admit(&old, 2);
    drop(old);
    let store = UserSendStore::open(dir.path()).unwrap();
    let ledger = ChatSendLedger::default();
    for record in store.queued("tab").unwrap() {
        ledger.restore_user_record(&record);
        assert!(!store.eligible(&record));
        let carried = store
            .carry_held_send(&record, next_launch())
            .unwrap()
            .unwrap();
        assert!(ledger.rebind_queued_user_authority(
            &record.send_id,
            &record.authority,
            &carried.authority
        ));
        assert!(store.eligible(&carried));
        assert!(!store.ready_launch_is_latched(&carried.authority));
        assert_eq!(store.prepared(&carried).unwrap(), "payload");
        assert_eq!(
            ledger.status(&record.send_id).unwrap().state,
            ChatSendState::Pending
        );
    }
    assert!(store.latch_ready_launch(&next_launch()));
    let mut written = Vec::new();
    for record in store.queued("tab").unwrap() {
        assert!(store.ready_launch_is_latched(&record.authority));
        assert!(store.claim(&record, "session").unwrap());
        written.push(record.send_id.clone());
        assert!(!store.claim(&record, "session").unwrap());
        store.finish_handoff(&record, false).unwrap();
        assert!(store.complete(&record).unwrap());
    }
    assert_eq!(written, [first.send_id, second.send_id]);
    assert!(store.queued("tab").unwrap().is_empty());
    assert_eq!(store.receipts("tab").unwrap().len(), 2);
}

#[test]
fn claimed_send_survives_restart_as_uncertain_without_carry_or_retry() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let record = admit(&store, 1);
    assert!(store.claim(&record, "session").unwrap());
    let reopened = UserSendStore::open(dir.path()).unwrap();
    let record = reopened.head("tab").unwrap().unwrap();
    assert!(reopened
        .carry_held_send(&record, next_launch())
        .unwrap()
        .is_none());
    let ledger = ChatSendLedger::default();
    ledger.restore_user_record(&record);
    assert_eq!(
        ledger.status(&record.send_id).unwrap().state,
        ChatSendState::Unknown
    );
    assert!(!reopened.claim(&record, "session").unwrap());
}

#[test]
fn proven_zero_write_handoff_follows_a_replacement_launch_as_pre_readiness_input() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let reservation = store
        .reserve(authority(), Some("token".into()), || {
            (uuid::Uuid::new_v4().to_string(), 1)
        })
        .unwrap();
    let record = store
        .admit_with_follow_up(&reservation, "payload", "payload", None, None, false)
        .unwrap();
    drop(reservation);
    assert!(store.claim(&record, "session").unwrap());
    store.finish_handoff(&record, true).unwrap();
    let record = store.head("tab").unwrap().unwrap();
    assert_eq!(record.state, UserSendState::Queued);
    assert!(record.handoff_session.is_none());
    let ledger = ChatSendLedger::default();
    ledger.restore_user_record(&record);
    let carried = store
        .carry_held_send(&record, next_launch())
        .unwrap()
        .unwrap();
    assert!(carried.held_before_readiness);
    assert!(ledger.rebind_queued_user_authority(
        &record.send_id,
        &record.authority,
        &carried.authority
    ));
    assert!(store.claim(&carried, "session").unwrap());
    ledger.mark_user_written(&carried.send_id).unwrap();
    assert!(ledger.has_held_user_send_before(&carried.authority, 2));
}

#[test]
fn ended_session_receipt_survives_restart_and_restores_the_correlated_failure() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let record = admit(&store, 1);
    assert!(store
        .carry_held_send(
            &record,
            UserSendAuthority {
                observed_session_id: Some("different-session".into()),
                ..next_launch()
            }
        )
        .unwrap()
        .is_none());
    assert!(store.discard_queued_from_ended_launch(&record).unwrap());
    let reopened = UserSendStore::open(dir.path()).unwrap();
    assert!(reopened.queued("tab").unwrap().is_empty());
    let ledger = ChatSendLedger::default();
    ledger.restore_user_receipt(&reopened.receipts("tab").unwrap()[0]);
    let status = ledger.reconcile("tab").pop().unwrap();
    assert_eq!(status.send_id, record.send_id);
    assert_eq!(status.correlation_id.as_deref(), Some("token-1"));
    assert_eq!(status.state, ChatSendState::Failed);
    assert_eq!(status.error.as_deref(), Some("launch_ended_before_write"));
}

#[test]
fn completed_payload_receipts_restore_uncertainty_and_late_delivery_without_replay() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let record = admit(&store, 1);
    assert!(store.claim(&record, "session").unwrap());
    store.finish_handoff(&record, false).unwrap();
    store.complete(&record).unwrap();
    let reopened = UserSendStore::open(dir.path()).unwrap();
    let ledger = ChatSendLedger::default();
    ledger.restore_user_receipt(&reopened.receipts("tab").unwrap()[0]);
    assert_eq!(
        ledger.status(&record.send_id).unwrap().state,
        ChatSendState::Unknown
    );
    assert!(reopened.queued("tab").unwrap().is_empty());
    reopened
        .record_delivered("tab", &[record.send_id.clone()])
        .unwrap();
    let restarted = UserSendStore::open(dir.path()).unwrap();
    let ledger = ChatSendLedger::default();
    ledger.restore_user_receipt(&restarted.receipts("tab").unwrap()[0]);
    assert_eq!(
        ledger.status(&record.send_id).unwrap().state,
        ChatSendState::Sent
    );
}

#[test]
fn fresh_restart_settles_two_held_sends_durably_and_never_carries_them_back() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let first = admit(&store, 1);
    let second = admit(&store, 2);
    let late = store
        .reserve(authority(), Some("late".into()), || {
            (uuid::Uuid::new_v4().to_string(), 3)
        })
        .unwrap();
    let ended = store
        .end_held_for_fresh_restart("tab", Some("old-nonce".into()), &[])
        .unwrap();
    assert_eq!(
        ended
            .iter()
            .map(|record| record.send_id.as_str())
            .collect::<Vec<_>>(),
        [first.send_id.as_str(), second.send_id.as_str()]
    );
    assert!(!store.claim(&first, "session").unwrap());
    assert!(!store.claim(&second, "session").unwrap());
    assert!(store.admit(&late, "late", "late", None).is_err());
    drop(late);
    drop(store);
    let reopened = UserSendStore::open(dir.path()).unwrap();
    assert!(reopened.queued("tab").unwrap().is_empty());
    let ledger = ChatSendLedger::default();
    let receipts = reopened.receipts("tab").unwrap();
    assert_eq!(receipts.len(), 2);
    for receipt in receipts {
        ledger.restore_user_receipt(&receipt);
        let status = ledger.status(&receipt.send_id).unwrap();
        assert_eq!(status.state, ChatSendState::Failed);
        assert_eq!(status.error.as_deref(), Some("launch_ended_before_write"));
        assert!(status.correlation_id.is_some());
    }
    assert!(reopened
        .carry_held_send(&first, next_launch())
        .unwrap()
        .is_none());
    assert!(reopened
        .carry_held_send(&second, next_launch())
        .unwrap()
        .is_none());
}

#[test]
fn fresh_settlement_never_classifies_claimed_written_or_uncertain_input_as_zero_write() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let queued = admit(&store, 1);
    let mut index = UserIndex::default();
    for (sequence, state) in [
        UserSendState::Queued,
        UserSendState::Claimed,
        UserSendState::Uncertain,
        UserSendState::BodyLease,
    ]
    .into_iter()
    .enumerate()
    {
        index.records.push(UserSendRecord {
            send_id: format!("state-{sequence}"),
            state,
            ..queued.clone()
        });
    }
    index.records.push(UserSendRecord {
        send_id: "possible-write".into(),
        ..queued.clone()
    });
    index.records.push(UserSendRecord {
        send_id: "possible-handoff".into(),
        handoff_session: Some("session".into()),
        ..queued
    });
    let ended = settle_fresh_launch(&mut index, &["possible-write".into()]);
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].send_id, "state-0");
    assert_eq!(index.records.len(), 5);
    assert_eq!(index.receipts.len(), 1);
    assert_eq!(index.receipts[0].state, ReceiptState::Failed);
}

#[test]
fn fresh_restart_does_not_publish_zero_write_receipts_when_persistence_fails() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let record = admit(&store, 1);
    store.set_persistence_boundary(Some(Arc::new(|_| {
        Err("injected persistence failure".into())
    })));
    assert!(store
        .end_held_for_fresh_restart("tab", Some("old-nonce".into()), &[])
        .is_err());
    let reopened = UserSendStore::open(dir.path()).unwrap();
    assert!(reopened.receipts("tab").unwrap().is_empty());
    assert_eq!(reopened.queued("tab").unwrap()[0].send_id, record.send_id);
}

#[test]
fn degraded_settlement_survives_restart_and_cannot_flush_on_late_readiness() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let first = admit(&store, 1);
    let second = admit(&store, 2);
    let ledger = ChatSendLedger::default();
    for record in [&first, &second] {
        ledger.restore_user_record(record);
        assert!(store.discard_queued_for_degraded_launch(record).unwrap());
    }
    drop(store);
    let reopened = UserSendStore::open(dir.path()).unwrap();
    assert!(reopened.queued("tab").unwrap().is_empty());
    assert!(reopened.latch_ready_launch(&authority()));
    for record in [&first, &second] {
        assert!(!reopened.claim(record, "session").unwrap());
        assert!(reopened
            .carry_held_send(record, next_launch())
            .unwrap()
            .is_none());
    }
    for receipt in reopened.receipts("tab").unwrap() {
        ledger.restore_user_receipt(&receipt);
        let status = ledger.status(&receipt.send_id).unwrap();
        assert_eq!(status.state, ChatSendState::Failed);
        assert_eq!(
            status.error.as_deref(),
            Some("launch_degraded_before_write")
        );
        assert!(status.correlation_id.is_some());
    }
}

#[test]
fn degraded_settlement_cannot_cancel_claimed_input_or_publish_before_durability() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let claimed = admit(&store, 1);
    assert!(store.claim(&claimed, "session").unwrap());
    assert!(!store.discard_queued_for_degraded_launch(&claimed).unwrap());
    let queued = admit(&store, 2);
    store.set_persistence_boundary(Some(Arc::new(|_| Err("injected failure".into()))));
    assert!(store.discard_queued_for_degraded_launch(&queued).is_err());
    let reopened = UserSendStore::open(dir.path()).unwrap();
    assert!(reopened.receipts("tab").unwrap().is_empty());
    assert_eq!(reopened.queued("tab").unwrap()[0].send_id, queued.send_id);
}
