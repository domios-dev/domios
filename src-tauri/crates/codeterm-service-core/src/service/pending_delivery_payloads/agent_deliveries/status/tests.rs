use super::*;

fn drain(store: &AgentDeliveryStore, tab_id: &str) -> Vec<String> {
    let mut written = Vec::new();
    while let Some(record) = store.pending_head(tab_id).unwrap() {
        assert!(store.claim(tab_id, &record.id).unwrap());
        written.push(record.id.clone());
        assert!(store.complete_written(tab_id, &record.id).unwrap());
    }
    assert!(!store.has_pending(tab_id).unwrap());
    written
}

#[test]
fn crash_after_receipt_257_recovers_retention_and_flushes_later_sends() {
    let dir = tempfile::tempdir().unwrap();
    let original = AgentDeliveryStore::open(dir.path());
    let tab_id = "own-tab";
    let tab = original.tab(tab_id);
    {
        let mut loaded = tab.index.lock();
        let index = original.load(&tab, &mut loaded).unwrap();
        index.next_sequence = MAX_INSPECTION_RECEIPTS as u64 + 1;
        original.persist(&tab, index).unwrap();
    }
    let inspection = tab.dir.join("inspection");
    fs::create_dir_all(&inspection).unwrap();
    let mut oldest = None;
    for sequence in 1..=MAX_INSPECTION_RECEIPTS as u64 {
        let id = uuid::Uuid::new_v4().to_string();
        let receipt = status(tab_id, &id, sequence, ChatSendState::Unknown, None);
        fs::write(
            inspection.join(format!("{id}.json")),
            serde_json::to_vec(&receipt).unwrap(),
        )
        .unwrap();
        if sequence == 1 {
            oldest = Some(id);
        }
    }
    let crashed = original.enqueue(tab_id, "already written", None).unwrap();
    let second = original.enqueue(tab_id, "second", None).unwrap();
    let third = original.enqueue(tab_id, "third", None).unwrap();
    assert!(original.claim(tab_id, &crashed).unwrap());
    publish_inspection(
        &tab,
        &status(
            tab_id,
            &crashed,
            MAX_INSPECTION_RECEIPTS as u64 + 1,
            ChatSendState::Unknown,
            None,
        ),
        false,
    )
    .unwrap();
    assert_eq!(
        fs::read_dir(&inspection).unwrap().count(),
        MAX_INSPECTION_RECEIPTS + 1
    );
    drop(original);

    let restarted = AgentDeliveryStore::open(dir.path());
    assert_eq!(drain(&restarted, tab_id), [second, third]);
    assert_eq!(
        restarted
            .inspect(&send_id(tab_id, &crashed))
            .unwrap()
            .unwrap()
            .state,
        ChatSendState::Unknown
    );
    assert!(fs::read_dir(&inspection).unwrap().count() <= MAX_INSPECTION_RECEIPTS);
    assert!(restarted
        .inspect(&send_id(tab_id, &oldest.unwrap()))
        .unwrap()
        .is_none());
    for entry in fs::read_dir(&inspection).unwrap() {
        let path = entry.unwrap().path();
        let mut receipt: ChatSendStatus =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        if receipt.send_id != crashed {
            receipt.sequence += 1000;
            fs::write(path, serde_json::to_vec(&receipt).unwrap()).unwrap();
        }
    }
    let next = restarted.enqueue(tab_id, "after eviction", None).unwrap();
    assert_eq!(drain(&restarted, tab_id), [next]);
    assert!(!inspection.join(format!("{crashed}.json")).exists());
    assert_eq!(
        restarted
            .inspect(&send_id(tab_id, &crashed))
            .unwrap()
            .unwrap()
            .state,
        ChatSendState::Unknown
    );
    let mut recovery = restarted
        .pending_receipts(tab_id)
        .unwrap()
        .into_iter()
        .find(|receipt| receipt.id == crashed)
        .unwrap();
    recovery.status = Some(AgentDeliveryReceiptStatus::Delivered);
    restarted.complete_receipt(&mut recovery).unwrap();
    assert_eq!(
        restarted
            .inspect(&send_id(tab_id, &crashed))
            .unwrap()
            .unwrap()
            .state,
        ChatSendState::Sent
    );
    assert!(drain(&restarted, tab_id).is_empty());
}

#[test]
fn unreadable_receipt_and_prune_failure_cannot_block_written_completion() {
    let dir = tempfile::tempdir().unwrap();
    let store = AgentDeliveryStore::open(dir.path());
    let tab_id = "own-tab";
    let first = store.enqueue(tab_id, "first", None).unwrap();
    let second = store.enqueue(tab_id, "second", None).unwrap();
    let inspection = store.tab(tab_id).dir.join("inspection");
    fs::create_dir_all(&inspection).unwrap();
    let unreadable = inspection.join(format!("{}.json", uuid::Uuid::new_v4()));
    fs::create_dir(&unreadable).unwrap();
    fs::write(
        inspection.join(format!("{}.json", uuid::Uuid::new_v4())),
        b"invalid",
    )
    .unwrap();
    assert_eq!(drain(&store, tab_id), [first, second]);
    assert!(unreadable.is_dir());
}

#[test]
fn inspection_publication_failure_cannot_hold_a_written_claim() {
    let dir = tempfile::tempdir().unwrap();
    let store = AgentDeliveryStore::open(dir.path());
    let tab_id = "own-tab";
    let first = store.enqueue(tab_id, "first", None).unwrap();
    let second = store.enqueue(tab_id, "second", None).unwrap();
    fs::create_dir(store.tab(tab_id).dir.join("inspection.next.json")).unwrap();
    assert_eq!(drain(&store, tab_id), [first, second]);
    assert!(!store.root.join("locators").exists());
    assert_eq!(
        fs::read_dir(store.tab(tab_id).dir.join("inspection"))
            .unwrap()
            .count(),
        0
    );
    assert!(store.records(tab_id).unwrap().is_empty());
    let entries = fs::read_dir(store.tab(tab_id).dir.clone()).unwrap().count();
    let later = store.enqueue(tab_id, "later", None).unwrap();
    assert_eq!(drain(&store, tab_id), [later]);
    assert_eq!(
        fs::read_dir(store.tab(tab_id).dir.clone()).unwrap().count(),
        entries
    );
    assert_eq!(fs::read_dir(&store.root).unwrap().count(), 1);
}

#[test]
fn self_locating_id_never_locks_or_reads_foreign_outboxes() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(AgentDeliveryStore::open(dir.path()));
    let own = store.enqueue("own-tab", "own payload", None).unwrap();
    store
        .enqueue("foreign-tab", "foreign payload", None)
        .unwrap();
    let foreign = store.tab("foreign-tab");
    let guard = foreign.index.lock();
    let worker_store = store.clone();
    let unknown = send_id("own-tab", &uuid::Uuid::new_v4().to_string());
    let own = send_id("own-tab", &own);
    let (send, receive) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        send.send((worker_store.inspect(&own), worker_store.inspect(&unknown)))
            .unwrap();
    });
    let result = receive.recv_timeout(std::time::Duration::from_secs(5));
    drop(guard);
    worker.join().unwrap();
    let (known, unknown) = result.expect("status lookup touched the locked foreign outbox");
    assert_eq!(known.unwrap().unwrap().tab_id, "own-tab");
    assert!(unknown.unwrap().is_none());
    assert!(store.has_pending("own-tab").unwrap());
    let own_record = store.pending_head("own-tab").unwrap().unwrap();
    assert!(store
        .inspect(&send_id("foreign-tab", &own_record.id))
        .unwrap()
        .is_none());
    assert!(store
        .inspect("agent-pty:not-base64:not-uuid")
        .unwrap()
        .is_none());
    assert!(store
        .inspect(&send_id("absent-tab", &uuid::Uuid::new_v4().to_string()))
        .unwrap()
        .is_none());
}
