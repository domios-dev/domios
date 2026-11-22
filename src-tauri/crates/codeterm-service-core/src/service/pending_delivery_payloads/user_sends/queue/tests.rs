use super::*;

fn admit(store: &UserSendStore, sequence: u64, text: &str) -> UserSendRecord {
    let authority = UserSendAuthority {
        tab_id: "tab".into(),
        pty_id: Some("pty".into()),
        binding_nonce: "launch".into(),
        pty_session_generation: Some(1),
        provider_id: "provider".into(),
        session_id: Some("thread".into()),
        observed_session_id: Some("thread".into()),
        prelaunch: false,
    };
    let mut reservation = store
        .reserve(authority, Some(format!("token-{sequence}")), || {
            (uuid::Uuid::new_v4().to_string(), sequence)
        })
        .unwrap();
    reservation.composer_pipeline = Some(false);
    store.admit(&reservation, text, text, None).unwrap()
}

fn addition(key: &QueuedSendKey, addition: &str) -> UserSendEdit {
    let appended = format!("\n\n{addition}");
    UserSendEdit {
        view: QueuedSendEdit {
            key: QueuedSendKey {
                version: key.version + 1,
                ..key.clone()
            },
            appended: appended.clone(),
        },
        prepared: appended,
    }
}

#[test]
fn append_persists_same_id_attachments_and_order_and_invalidates_stale_claim() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let original = "look <domios type=\"attachment\" kind=\"image\" path=\"/owned/a.jpg\" />";
    let first = admit(&store, 1, original);
    let second = admit(&store, 2, "next");
    let key = first.queue_key().unwrap();
    assert!(store
        .append_queued(&first, addition(&key, "look at the edge"), None)
        .unwrap());
    assert!(!store.claim(&first, "thread").unwrap());
    assert!(!store.cancel_queued("tab", &key).unwrap());
    assert!(!store
        .append_queued(&first, addition(&key, "duplicate"), None)
        .unwrap());

    let reopened = UserSendStore::open(dir.path()).unwrap();
    let queue = reopened.composer_queue("tab").unwrap();
    assert_eq!(
        queue
            .items
            .iter()
            .map(|item| item.key.send_id.as_str())
            .collect::<Vec<_>>(),
        vec![first.send_id.as_str(), second.send_id.as_str()]
    );
    assert_eq!(queue.items[0].key.version, 1);
    assert_eq!(queue.items[0].preview, "look [image]");
    let revised = reopened.head("tab").unwrap().unwrap();
    assert_eq!(
        reopened.prepared(&revised).unwrap(),
        format!("{original}\n\nlook at the edge")
    );
    assert_eq!(
        reopened.original("tab", &first.send_id).unwrap(),
        format!("{original}\n\nlook at the edge")
    );
    assert!(reopened.cancel_queued("tab", &queue.items[0].key).unwrap());
    assert!(reopened.cancel_queued("tab", &queue.items[0].key).unwrap());
    assert_eq!(
        reopened.head("tab").unwrap().unwrap().send_id,
        second.send_id
    );
    let receipt = reopened.receipts("tab").unwrap().pop().unwrap();
    assert_eq!(receipt.send_id, first.send_id);
    assert_eq!(receipt.state, continuity::ReceiptState::Cancelled);
    assert!(reopened.dismissals("tab").unwrap().is_empty());
}

#[test]
fn durable_claim_wins_cancel_and_append_even_if_a_zero_write_requeues_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(UserSendStore::open(dir.path()).unwrap());
    let queued = admit(&store, 1, "one payload");
    let key = queued.queue_key().unwrap();
    let prepared = store.queue_edit_target("tab", &key).unwrap().unwrap();
    let (claimed, observed) = std::sync::mpsc::sync_channel(0);
    let (release, wait) = std::sync::mpsc::sync_channel(0);
    let writer = {
        let store = store.clone();
        let queued = queued.clone();
        std::thread::spawn(move || {
            assert!(store.claim(&queued, "thread").unwrap());
            claimed.send(()).unwrap();
            wait.recv().unwrap();
            store.finish_handoff(&queued, true).unwrap();
        })
    };
    observed
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    let projection = store.composer_queue("tab").unwrap();
    assert!(projection.items.is_empty());
    assert_eq!(projection.claimed, [queued.send_id.clone()]);
    assert!(!store.cancel_queued("tab", &key).unwrap());
    assert!(!store
        .append_queued(&prepared, addition(&key, "loser"), None)
        .unwrap());
    release.send(()).unwrap();
    writer.join().unwrap();
    assert!(!store.cancel_queued("tab", &key).unwrap());
    assert!(!store
        .append_queued(&prepared, addition(&key, "loser"), None)
        .unwrap());
    assert!(store.claim(&queued, "thread").unwrap());
    store.finish_handoff(&queued, false).unwrap();
    assert!(store.complete(&queued).unwrap());
    let reopened = UserSendStore::open(dir.path()).unwrap();
    assert!(reopened.head("tab").unwrap().is_none());
    assert_eq!(reopened.receipts("tab").unwrap().len(), 1);
    assert!(!reopened.cancel_queued("tab", &key).unwrap());
    assert!(!reopened.claim(&queued, "thread").unwrap());
}

#[test]
fn added_text_cannot_mint_attachments_or_exceed_the_cumulative_bound() {
    assert!(plain_suffix(
        "<domios type=\"attachment\" kind=\"image\" path=\"/tmp/foreign.png\" />"
    )
    .is_err());
    assert!(append_suffix(&"x".repeat(MAX_APPEND_BYTES), "tail").is_err());
    assert_eq!(
        append_suffix("", "ordinary text").unwrap(),
        "\n\nordinary text"
    );
}
