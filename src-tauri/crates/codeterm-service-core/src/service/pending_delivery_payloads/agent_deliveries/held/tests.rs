use super::*;

#[test]
fn cancellation_and_claim_are_mutually_exclusive_under_the_durable_lock() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(AgentDeliveryStore::open(dir.path()));
    let id = store.enqueue("tab", "opaque payload", None).unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let worker = {
        let store = store.clone();
        let id = id.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            store.claim("tab", &id).unwrap()
        })
    };
    barrier.wait();
    let cancelled = store.refuse_held("tab", &id, "cancelled_before_write");
    let claimed = worker.join().unwrap();
    assert_eq!(cancelled.is_ok_and(|changed| changed), !claimed);
    if claimed {
        assert!(store
            .refuse_held("tab", &id, "cancelled_before_write")
            .is_err());
    } else {
        assert!(!store.claim("tab", &id).unwrap());
        drop(store);
        let restored = AgentDeliveryStore::open(dir.path());
        assert!(!restored.claim("tab", &id).unwrap());
        assert_eq!(
            restored
                .inspect(&status::send_id("tab", &id))
                .unwrap()
                .unwrap()
                .error
                .as_deref(),
            Some("cancelled_before_write")
        );
    }
}

#[test]
fn refused_send_keeps_its_proof_when_receipt_publication_fails() {
    let dir = tempfile::tempdir().unwrap();
    let store = AgentDeliveryStore::open(dir.path());
    let id = store.enqueue("tab", "opaque payload", None).unwrap();
    assert!(store
        .refuse_held("tab", &id, "cancelled_before_write")
        .unwrap());
    fs::create_dir(store.tab("tab").dir.join("inspection.next.json")).unwrap();
    assert!(store.discard_expired("tab", &id).is_err());
    assert!(!store.claim("tab", &id).unwrap());
    drop(store);
    let restored = AgentDeliveryStore::open(dir.path());
    assert!(!restored.claim("tab", &id).unwrap());
    assert_eq!(
        restored
            .inspect(&status::send_id("tab", &id))
            .unwrap()
            .unwrap()
            .error
            .as_deref(),
        Some("cancelled_before_write")
    );
}
