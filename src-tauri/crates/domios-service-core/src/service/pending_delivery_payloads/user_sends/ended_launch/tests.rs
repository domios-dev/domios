use super::*;

fn admitted(store: &UserSendStore) -> UserSendRecord {
    let reservation = store
        .reserve(
            UserSendAuthority {
                tab_id: "tab".into(),
                pty_id: Some("pty-a".into()),
                binding_nonce: "launch-a".into(),
                pty_session_generation: Some(1),
                provider_id: "opaque-provider".into(),
                session_id: Some("session-a".into()),
                observed_session_id: Some("session-a".into()),
                prelaunch: false,
            },
            Some("client-token".into()),
            || (uuid::Uuid::new_v4().to_string(), 1),
        )
        .unwrap();
    store
        .admit(&reservation, "opaque-payload", "opaque-payload", None)
        .unwrap()
}

#[test]
fn ended_head_failure_requires_zero_write_in_both_store_and_ledger() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let mut record = admitted(&store);
    for state in [
        UserSendState::Queued,
        UserSendState::Claimed,
        UserSendState::Uncertain,
        UserSendState::BodyLease,
    ] {
        for handoff in [None, Some(String::from("session-a"))] {
            for possibly_written in [false, true] {
                record.state = state;
                record.handoff_session = handoff.clone();
                assert_eq!(
                    ended_head_receipt(&record, possibly_written),
                    if state == UserSendState::Queued && handoff.is_none() && !possibly_written {
                        ReceiptState::Failed
                    } else {
                        ReceiptState::Uncertain
                    }
                );
            }
        }
    }
}

#[test]
fn changed_settlement_refreshes_then_removes_the_never_written_head_durably() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let snapshot = admitted(&store);
    let mut changed = snapshot.authority.clone();
    changed.pty_session_generation = Some(2);
    let current = store
        .rebind_queued_authority(&snapshot, changed)
        .unwrap()
        .unwrap();
    assert!(!store.discard_queued_from_ended_launch(&snapshot).unwrap());
    assert_eq!(
        store.settle_ended_head(&snapshot, false).unwrap(),
        EndedHeadSettlement::Refresh
    );
    assert_eq!(
        store.settle_ended_head(&current, false).unwrap(),
        EndedHeadSettlement::Settled(ReceiptState::Failed)
    );
    let reopened = UserSendStore::open(dir.path()).unwrap();
    assert!(reopened.head("tab").unwrap().is_none());
    assert_eq!(
        reopened.receipts("tab").unwrap()[0].state,
        ReceiptState::Failed
    );
}

#[test]
fn ended_claim_terminalizes_as_uncertainty_without_replay() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let record = admitted(&store);
    assert!(store.claim(&record, "session-a").unwrap());
    assert_eq!(
        store.settle_ended_head(&record, false).unwrap(),
        EndedHeadSettlement::Settled(ReceiptState::Uncertain)
    );
    let reopened = UserSendStore::open(dir.path()).unwrap();
    assert!(reopened.head("tab").unwrap().is_none());
    assert_eq!(
        reopened.receipts("tab").unwrap()[0].state,
        ReceiptState::Uncertain
    );
}

#[test]
fn settlement_failure_never_publishes_a_failure_receipt_or_removes_input() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let record = admitted(&store);
    store.set_persistence_boundary(Some(Arc::new(|_| Err("injected durable failure".into()))));
    assert!(store.settle_ended_head(&record, false).is_err());
    assert_eq!(store.head("tab").unwrap().unwrap().send_id, record.send_id);
    assert!(store.receipts("tab").unwrap().is_empty());
}
