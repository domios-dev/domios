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
