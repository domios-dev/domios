use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::session_persist;
use crate::service::test_fixture::ServiceFixture;

fn replacement_fixture(payload: &str) -> (ServiceFixture, String, PendingDeliveryRecord) {
    let (mut mux, tab) = MultiplexerState::new();
    let original = record(&digest(payload));
    mux.get_tab_mut(&tab).unwrap().pending_delivery = Some(original.clone());
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .pending_delivery_payloads
        .write(&original, payload)
        .unwrap();
    (fixture, tab, original)
}

#[test]
fn replacement_keeps_previous_payload_until_persistence_completes() {
    let (fixture, tab, original) = replacement_fixture("opaque-before-commit");
    let original_path = fixture
        .service
        .pending_delivery_payloads
        .path_for_test(&original);
    let (started, release, worker) =
        session_persist::persistence_test_support::install_blocked_test_worker(&fixture.service);
    let result = std::thread::scope(|scope| {
        let replacement = scope.spawn(|| {
            fixture.service.create_pending_delivery(
                &tab,
                PendingDeliveryChannel::InteractiveInput,
                "opaque-after-commit",
            )
        });
        started
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let retained_before_commit = original_path.exists();
        release.send(()).unwrap();
        let result = replacement.join().unwrap();
        assert!(retained_before_commit);
        result
    });
    session_persist::persistence_test_support::stop_installed_test_worker(&worker);
    result.unwrap();
    assert!(!original_path.exists());
}

#[test]
fn replacement_persist_failure_preserves_exact_payload_for_durable_snapshot() {
    let payload = "\topaque-persist-failure-\u{03bb}\r\n";
    let (fixture, tab, original) = replacement_fixture(payload);
    let storage = fixture.install_seeded_storage_desk();
    let adapter = crate::session::store::StorageDeskAdapter::from_storage(storage);
    session_persist::set_storage_unavailable(&fixture.service, "injected persist failure".into())
        .unwrap();

    let error = fixture
        .service
        .create_pending_delivery(
            &tab,
            PendingDeliveryChannel::InteractiveInput,
            "opaque-replacement-after-failure",
        )
        .unwrap_err();
    assert!(error.to_string().contains("injected persist failure"));
    let durable = MultiplexerState::from_session_snapshot(&adapter.snapshot().unwrap());
    let restored = durable
        .get_tab(&tab)
        .unwrap()
        .pending_delivery
        .as_ref()
        .unwrap();
    assert_eq!(restored.record_id(), original.record_id());
    let reopened = PendingDeliveryPayloadStore::open(
        fixture.temp.path(),
        &live_payload_keys(&durable.snapshot()),
    )
    .unwrap();
    assert_eq!(reopened.resolve(restored).unwrap(), payload);
    session_persist::stop_worker(&fixture.service).unwrap();
}

#[test]
fn committed_replacement_preserves_identical_sibling_and_reclaims_unused_legacy_payload() {
    for replacement in ["opaque-shared-commit", "opaque-distinct-commit"] {
        let payload = "opaque-shared-commit";
        let (fixture, tab, original) = replacement_fixture(payload);
        let workspace = fixture.service.mux.read().list_workspaces()[0].id.clone();
        let sibling = fixture.service.mux.write().create_tab(&workspace).unwrap();
        let sibling_record = record(&original.content_digest);
        fixture
            .service
            .mux
            .write()
            .get_tab_mut(&sibling)
            .unwrap()
            .pending_delivery = Some(sibling_record.clone());
        let legacy_path = fixture
            .service
            .pending_delivery_payloads
            .dir
            .join(&original.content_digest);
        fs::write(&legacy_path, payload).unwrap();
        let storage = fixture.install_seeded_storage_desk();
        let adapter = crate::session::store::StorageDeskAdapter::from_storage(storage);

        fixture
            .service
            .create_pending_delivery(&tab, PendingDeliveryChannel::InteractiveInput, replacement)
            .unwrap();
        let durable = MultiplexerState::from_session_snapshot(&adapter.snapshot().unwrap());
        let current = durable
            .get_tab(&tab)
            .unwrap()
            .pending_delivery
            .as_ref()
            .unwrap();
        let store = &fixture.service.pending_delivery_payloads;
        assert_ne!(current.record_id(), original.record_id());
        assert_eq!(store.resolve(current).unwrap(), replacement);
        assert!(!store.path_for_test(&original).exists());
        assert_eq!(store.resolve(&sibling_record).unwrap(), payload);
        fixture.service.discard_pending_delivery(&sibling).unwrap();
        assert_eq!(legacy_path.exists(), replacement == payload);
        fixture.service.discard_pending_delivery(&tab).unwrap();
        assert!(!legacy_path.exists());
        session_persist::stop_worker(&fixture.service).unwrap();
    }
}

#[test]
fn last_legacy_record_terminal_cleanup_removes_payload_without_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let payload = "opaque-legacy-terminal";
    let original = record(&digest(payload));
    let store = PendingDeliveryPayloadStore::open(temp.path(), &HashSet::new()).unwrap();
    let legacy_path = store.dir.join(&original.content_digest);
    fs::write(&legacy_path, payload).unwrap();
    let mut terminal = Some(original.clone());
    terminal.as_mut().unwrap().confirm().unwrap();

    store
        .free_terminal_pair(&mut terminal, "confirmed", &HashSet::new())
        .unwrap();

    assert!(terminal.is_none());
    assert!(!legacy_path.exists());
    assert!(matches!(
        store.resolve(&original),
        Err(PayloadStoreError::Missing { .. })
    ));
}

#[test]
fn last_legacy_record_replacement_removes_payload_without_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let payload = "opaque-legacy-replaced";
    let original = record(&digest(payload));
    let replacement = record(&digest("opaque-new-owner"));
    let store = PendingDeliveryPayloadStore::open(temp.path(), &HashSet::new()).unwrap();
    let legacy_path = store.dir.join(&original.content_digest);
    fs::write(&legacy_path, payload).unwrap();
    store.write(&replacement, "opaque-new-owner").unwrap();

    store
        .retire_replaced(
            Some(&original),
            &replacement,
            &HashSet::from([
                payload_key(&replacement),
                replacement.content_digest.clone(),
            ]),
        )
        .unwrap();

    assert!(!legacy_path.exists());
    assert_eq!(store.resolve(&replacement).unwrap(), "opaque-new-owner");
}
