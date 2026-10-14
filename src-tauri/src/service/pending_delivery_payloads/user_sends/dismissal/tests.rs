use super::*;

fn admitted(store: &UserSendStore) -> UserSendRecord {
    let authority = UserSendAuthority {
        tab_id: "tab".into(),
        pty_id: Some("pty".into()),
        binding_nonce: "nonce".into(),
        pty_session_generation: Some(1),
        provider_id: "provider".into(),
        session_id: Some("session".into()),
        observed_session_id: Some("session".into()),
        prelaunch: false,
    };
    let ticket = store
        .reserve(authority, Some("token".into()), || {
            (uuid::Uuid::new_v4().to_string(), 1)
        })
        .unwrap();
    store.admit(&ticket, "opaque", "opaque", None).unwrap()
}

fn dismissal(record: &UserSendRecord) -> UserSendDismissal {
    UserSendDismissal {
        send_id: record.send_id.clone(),
        send: DismissedBubble {
            id: "bubble".into(),
            correlation_id: "token".into(),
            content: "opaque".into(),
            sent_at_ms: 42,
            echo: None,
        },
    }
}

#[test]
fn cancelled_head_cannot_be_claimed_and_dismissal_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let record = admitted(&store);
    store.cancel_or_dismiss("tab", dismissal(&record)).unwrap();
    assert!(!store.claim(&record, "session").unwrap());
    let restored = UserSendStore::open(dir.path()).unwrap();
    assert!(restored.records("tab").unwrap().is_empty());
    assert_eq!(restored.dismissals("tab").unwrap()[0].send.id, "bubble");
    assert!(!restored.claim(&record, "session").unwrap());
}

#[test]
fn claim_winner_keeps_handoff_and_never_replays_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let record = admitted(&store);
    assert!(store.claim(&record, "session").unwrap());
    store.cancel_or_dismiss("tab", dismissal(&record)).unwrap();
    assert_eq!(
        store.records("tab").unwrap()[0].state,
        UserSendState::Claimed
    );
    store.finish_handoff(&record, false).unwrap();
    assert!(store.complete(&record).unwrap());
    let restored = UserSendStore::open(dir.path()).unwrap();
    assert!(restored.queued("tab").unwrap().is_empty());
    assert_eq!(restored.dismissals("tab").unwrap().len(), 1);
    assert!(!restored.claim(&record, "session").unwrap());
}

#[test]
fn dismissed_claim_with_zero_write_proof_cannot_requeue() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let record = admitted(&store);
    assert!(store.claim(&record, "session").unwrap());
    store.cancel_or_dismiss("tab", dismissal(&record)).unwrap();
    store.finish_handoff(&record, true).unwrap();
    assert!(store.queued("tab").unwrap().is_empty());
    assert!(!store.claim(&record, "session").unwrap());
}

#[test]
fn simultaneous_cancel_and_claim_have_one_atomic_winner() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(UserSendStore::open(dir.path()).unwrap());
    let record = admitted(&store);
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let claimed = std::thread::scope(|scope| {
        let claiming = scope.spawn(|| {
            barrier.wait();
            store.claim(&record, "session").unwrap()
        });
        barrier.wait();
        store.cancel_or_dismiss("tab", dismissal(&record)).unwrap();
        claiming.join().unwrap()
    });
    let records = store.records("tab").unwrap();
    if claimed {
        assert_eq!(records[0].state, UserSendState::Claimed);
    } else {
        assert!(records.is_empty());
    }
    assert_eq!(store.dismissals("tab").unwrap().len(), 1);
    assert!(!store.claim(&record, "session").unwrap());
}

#[test]
fn failed_dismissal_persistence_never_reports_cancellation() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let record = admitted(&store);
    *store.persistence_boundary.lock() = Some(Arc::new(|_| Err("injected".into())));
    assert!(store.cancel_or_dismiss("tab", dismissal(&record)).is_err());
    let restored = UserSendStore::open(dir.path()).unwrap();
    assert_eq!(restored.records("tab").unwrap().len(), 1);
    assert!(restored.dismissals("tab").unwrap().is_empty());
}

#[test]
fn cancellation_during_preparation_prevents_late_admission() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let record = admitted(&store);
    let mut authority = record.authority.clone();
    authority.tab_id = "preparing".into();
    let ticket = store
        .reserve(authority, Some("preparing-token".into()), || {
            (uuid::Uuid::new_v4().to_string(), 2)
        })
        .unwrap();
    let mut receipt = dismissal(&record);
    receipt.send_id = "optimistic-bubble".into();
    receipt.send.correlation_id = "preparing-token".into();
    store.cancel_or_dismiss("preparing", receipt).unwrap();
    assert!(store.admit(&ticket, "opaque", "opaque", None).is_err());
    assert!(store.queued("preparing").unwrap().is_empty());
    assert_eq!(
        UserSendStore::open(dir.path())
            .unwrap()
            .dismissals("preparing")
            .unwrap()
            .len(),
        1
    );
}
