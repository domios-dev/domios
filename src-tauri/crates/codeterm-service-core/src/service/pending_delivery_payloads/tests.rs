use super::*;
use crate::multiplexer::types::{
    PendingDeliveryChannel, PendingDeliveryRecord, PendingDeliveryState,
};

fn record(digest: &str) -> PendingDeliveryRecord {
    PendingDeliveryRecord::new(digest.into(), PendingDeliveryChannel::InteractiveInput)
}

fn digest(payload: &str) -> String {
    format!("{:x}", Sha256::digest(payload.as_bytes()))
}

#[derive(Default)]
struct CapturedPayloadEvents(parking_lot::Mutex<Vec<(String, String)>>);

impl LogPayloadEventSink for CapturedPayloadEvents {
    fn emit(&self, event: &str, digest: &str) {
        self.0.lock().push((event.into(), digest.into()));
    }
}

#[test]
fn payload_store_logs_digest_only_via_sink() {
    let temp = tempfile::tempdir().unwrap();
    let payload = "payload-must-not-be-logged";
    let digest = digest(payload);
    let owner = record(&digest);
    let events = Arc::new(CapturedPayloadEvents::default());
    let store =
        PendingDeliveryPayloadStore::open_with_sink(temp.path(), &HashSet::new(), events.clone())
            .unwrap();

    store.write(&owner, payload).unwrap();
    assert_eq!(store.resolve(&owner).unwrap(), payload);

    let events = events.0.lock().clone();
    assert_eq!(
        events,
        vec![("write".into(), digest.clone()), ("resolve".into(), digest)]
    );
    assert!(events
        .iter()
        .all(|(event, captured_digest)| !event.contains(payload)
            && !captured_digest.contains(payload)));
}

#[test]
fn payload_store_round_trip_across_restart() {
    let temp = tempfile::tempdir().unwrap();
    let payload = "the exact fallback brief";
    let digest = digest(payload);
    let owner = record(&digest);
    let store = PendingDeliveryPayloadStore::open(temp.path(), &HashSet::new()).unwrap();
    store.write(&owner, payload).unwrap();
    let live = HashSet::from([payload_key(&owner)]);
    let reopened = PendingDeliveryPayloadStore::open(temp.path(), &live).unwrap();
    assert_eq!(reopened.resolve(&owner).unwrap(), payload);
}

#[test]
fn missing_and_tampered_payloads_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let payload = "private brief";
    let digest = digest(payload);
    let owner = record(&digest);
    let store = PendingDeliveryPayloadStore::open(temp.path(), &HashSet::new()).unwrap();
    assert_eq!(
        store.resolve(&owner),
        Err(PayloadStoreError::Missing {
            digest: digest.clone()
        })
    );
    store.write(&owner, payload).unwrap();
    fs::write(store.path_for_test(&owner), "tampered").unwrap();
    assert_eq!(
        store.resolve(&owner),
        Err(PayloadStoreError::DigestMismatch { digest })
    );
}

#[test]
fn orphan_sweep_is_idempotent() {
    let temp = tempfile::tempdir().unwrap();
    let live_payload = "live";
    let orphan_payload = "orphan";
    let live = digest(live_payload);
    let live_owner = record(&live);
    let orphan = digest(orphan_payload);
    let orphan_owner = record(&orphan);
    let store = PendingDeliveryPayloadStore::open(temp.path(), &HashSet::new()).unwrap();
    store.write(&live_owner, live_payload).unwrap();
    store.write(&orphan_owner, orphan_payload).unwrap();
    let live_set = HashSet::from([payload_key(&live_owner)]);
    store.sweep_orphans(&live_set).unwrap();
    store.sweep_orphans(&live_set).unwrap();
    assert_eq!(store.resolve(&live_owner).unwrap(), live_payload);
    assert_eq!(
        store.resolve(&orphan_owner),
        Err(PayloadStoreError::Missing { digest: orphan })
    );
}

#[test]
fn confirmed_terminal_frees_record_and_payload_together() {
    let temp = tempfile::tempdir().unwrap();
    let payload = "terminal fallback brief";
    let digest = digest(payload);
    let owner = record(&digest);
    let events = Arc::new(CapturedPayloadEvents::default());
    let store =
        PendingDeliveryPayloadStore::open_with_sink(temp.path(), &HashSet::new(), events.clone())
            .unwrap();
    store.write(&owner, payload).unwrap();
    let mut record = Some(owner.clone());

    record.as_mut().unwrap().confirm().unwrap();
    store
        .free_terminal_pair(&mut record, "confirmed", &HashSet::new())
        .unwrap();

    assert!(record.is_none());
    assert_eq!(
        store.resolve(&owner),
        Err(PayloadStoreError::Missing {
            digest: digest.clone()
        })
    );
    assert!(events.0.lock().contains(&("confirmed".into(), digest)));
}

#[test]
fn explicit_discard_frees_record_and_payload_together() {
    let temp = tempfile::tempdir().unwrap();
    let payload = "discarded fallback brief";
    let digest = digest(payload);
    let owner = record(&digest);
    let events = Arc::new(CapturedPayloadEvents::default());
    let store =
        PendingDeliveryPayloadStore::open_with_sink(temp.path(), &HashSet::new(), events.clone())
            .unwrap();
    store.write(&owner, payload).unwrap();
    let mut record = Some(owner.clone());

    store
        .free_terminal_pair(&mut record, "explicit_discard", &HashSet::new())
        .unwrap();

    assert!(record.is_none());
    assert_eq!(
        store.resolve(&owner),
        Err(PayloadStoreError::Missing {
            digest: digest.clone()
        })
    );
    assert!(events
        .0
        .lock()
        .contains(&("explicit_discard".into(), digest)));
}

#[test]
fn retry_exhaustion_frees_record_and_payload_together() {
    let temp = tempfile::tempdir().unwrap();
    let payload = "retry-exhausted fallback brief";
    let digest = digest(payload);
    let owner = record(&digest);
    let events = Arc::new(CapturedPayloadEvents::default());
    let store =
        PendingDeliveryPayloadStore::open_with_sink(temp.path(), &HashSet::new(), events.clone())
            .unwrap();
    store.write(&owner, payload).unwrap();
    let mut record = Some(owner.clone());
    record.as_mut().unwrap().mark_unconfirmed();

    store
        .free_terminal_pair(&mut record, "retry_exhausted", &HashSet::new())
        .unwrap();

    assert!(record.is_none());
    assert_eq!(
        store.resolve(&owner),
        Err(PayloadStoreError::Missing {
            digest: digest.clone()
        })
    );
    assert!(events
        .0
        .lock()
        .contains(&("retry_exhausted".into(), digest)));
}

#[test]
fn terminal_cleanup_failure_keeps_record_addressable() {
    let temp = tempfile::tempdir().unwrap();
    let store = PendingDeliveryPayloadStore::open(temp.path(), &HashSet::new()).unwrap();
    let mut record = Some(PendingDeliveryRecord::new(
        "invalid".into(),
        PendingDeliveryChannel::InteractiveInput,
    ));

    assert_eq!(
        store.free_terminal_pair(&mut record, "confirmed", &HashSet::new()),
        Err(PayloadStoreError::InvalidDigest)
    );
    assert!(record.is_some());
}

#[test]
fn superseding_payload_removes_prior_digest() {
    let temp = tempfile::tempdir().unwrap();
    let first = "first brief";
    let replacement = "replacement brief";
    let first_digest = digest(first);
    let first_owner = record(&first_digest);
    let replacement_digest = digest(replacement);
    let replacement_owner = record(&replacement_digest);
    let store = PendingDeliveryPayloadStore::open(temp.path(), &HashSet::new()).unwrap();
    store.write(&first_owner, first).unwrap();
    store.write(&replacement_owner, replacement).unwrap();
    store
        .retire_replaced(Some(&first_owner), &replacement_owner, &HashSet::new())
        .unwrap();

    assert_eq!(store.resolve(&replacement_owner).unwrap(), replacement);
    assert_eq!(
        store.resolve(&first_owner),
        Err(PayloadStoreError::Missing {
            digest: first_digest
        })
    );
}

#[test]
fn payload_permissions_are_restricted() {
    let temp = tempfile::tempdir().unwrap();
    let payload = "permission fixture";
    let digest = digest(payload);
    let owner = record(&digest);
    let store = PendingDeliveryPayloadStore::open(temp.path(), &HashSet::new()).unwrap();
    store.write(&owner, payload).unwrap();
    let path = store.path_for_test(&owner);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    #[cfg(windows)]
    {
        let mut command = std::process::Command::new("icacls");
        crate::process_util::no_window_std(&mut command);
        let output = command.arg(path).output().unwrap();
        let acl = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
        assert!(acl.contains(&std::env::var("USERNAME").unwrap().to_ascii_lowercase()));
        assert!(!acl.contains("builtin\\users"));
        assert!(!acl.contains("everyone:"));
    }
}

#[test]
fn write_and_replace_do_not_run_acl_setup_after_open() {
    let temp = tempfile::tempdir().unwrap();
    let first = "first payload";
    let second = "second payload";
    let first_digest = digest(first);
    let first_owner = record(&first_digest);
    let second_digest = digest(second);
    let second_owner = record(&second_digest);
    let store = PendingDeliveryPayloadStore::open(temp.path(), &HashSet::new()).unwrap();
    #[cfg(windows)]
    let calls_after_open = acl_setup_calls();

    store.write(&first_owner, first).unwrap();
    store.write(&second_owner, second).unwrap();
    store
        .retire_replaced(Some(&first_owner), &second_owner, &HashSet::new())
        .unwrap();

    #[cfg(windows)]
    assert_eq!(acl_setup_calls(), calls_after_open);
}

#[test]
fn restore_resolves_payload_into_launch_option() {
    let temp = tempfile::tempdir().unwrap();
    let payload = "restored exact fallback brief";
    let digest = digest(payload);
    let owner = record(&digest);
    let store = PendingDeliveryPayloadStore::open(temp.path(), &HashSet::new()).unwrap();
    store.write(&owner, payload).unwrap();
    let record = owner.clone();

    let launch_option = record
        .state()
        .needs_delivery()
        .then(|| store.resolve(&record))
        .transpose()
        .unwrap();

    assert_eq!(launch_option.as_deref(), Some(payload));
}

#[test]
fn restore_missing_payload_structured_error() {
    let temp = tempfile::tempdir().unwrap();
    let payload = "unavailable fallback brief";
    let digest = digest(payload);
    let owner = record(&digest);
    let store = PendingDeliveryPayloadStore::open(temp.path(), &HashSet::new()).unwrap();
    let record = owner.clone();
    let delivery_writes = std::sync::atomic::AtomicUsize::new(0);

    let error = store.resolve(&record).unwrap_err();

    assert_eq!(error, PayloadStoreError::Missing { digest });
    assert_eq!(record.state(), PendingDeliveryState::Pending);
    assert_eq!(
        delivery_writes.load(std::sync::atomic::Ordering::Relaxed),
        0
    );
}

#[test]
fn leakage_serialization_keeps_payload_out_of_snapshot() {
    let payload = "payload-must-never-enter-snapshots-or-logs";
    let digest = digest(payload);
    let owner = record(&digest);
    let (mut state, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    state.get_tab_mut(&tab_id).unwrap().pending_delivery = Some(owner.clone());
    let snapshot = state.to_session_snapshot();
    let serialized = serde_json::to_string(&snapshot).unwrap();

    assert!(serialized.contains(&digest));
    assert!(!serialized.contains(payload));
}

#[test]
fn live_digest_projection_retains_every_addressable_state() {
    let (mut state, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let payload = "retryable launch assignment";
    let digest = digest(payload);
    let owner = record(&digest);
    let record = owner.clone();
    state.get_tab_mut(&tab_id).unwrap().pending_delivery = Some(record);
    assert_eq!(
        live_payload_keys(&state.snapshot()),
        HashSet::from([payload_key(&owner), digest.clone()])
    );

    state
        .get_tab_mut(&tab_id)
        .unwrap()
        .pending_delivery
        .as_mut()
        .unwrap()
        .mark_ingested();
    assert_eq!(
        live_payload_keys(&state.snapshot()),
        HashSet::from([payload_key(&owner), digest.clone()])
    );

    state
        .get_tab_mut(&tab_id)
        .unwrap()
        .pending_delivery
        .as_mut()
        .unwrap()
        .mark_unconfirmed();
    assert_eq!(
        live_payload_keys(&state.snapshot()),
        HashSet::from([payload_key(&owner), digest.clone()])
    );

    state
        .get_tab_mut(&tab_id)
        .unwrap()
        .pending_delivery
        .as_mut()
        .unwrap()
        .confirm()
        .unwrap();
    assert_eq!(
        live_payload_keys(&state.snapshot()),
        HashSet::from([payload_key(&owner), digest])
    );
}

#[test]
fn identical_payload_sibling_survives_confirmation_and_restart() {
    let temp = tempfile::tempdir().unwrap();
    let payload = "opaque-identical-\u{03bb}\r\n\t";
    let first = record(&digest(payload));
    let second = record(&digest(payload));
    let store = PendingDeliveryPayloadStore::open(temp.path(), &HashSet::new()).unwrap();
    store.write(&first, payload).unwrap();
    store.write(&second, payload).unwrap();
    assert_ne!(store.path_for_test(&first), store.path_for_test(&second));
    let mut terminal = Some(first.clone());
    terminal.as_mut().unwrap().confirm().unwrap();
    store
        .free_terminal_pair(
            &mut terminal,
            "confirmed",
            &HashSet::from([second.content_digest.clone()]),
        )
        .unwrap();
    assert!(!store.path_for_test(&first).exists());
    assert!(store.path_for_test(&second).exists());
    let serialized = serde_json::to_string(&second).unwrap();
    let restored: PendingDeliveryRecord = serde_json::from_str(&serialized).unwrap();
    let reopened =
        PendingDeliveryPayloadStore::open(temp.path(), &HashSet::from([payload_key(&restored)]))
            .unwrap();
    assert_eq!(reopened.resolve(&restored).unwrap(), payload);
}

#[test]
fn replacing_identical_payload_owner_never_deletes_sibling() {
    let temp = tempfile::tempdir().unwrap();
    let payload = "opaque-shared-body";
    let first = record(&digest(payload));
    let second = record(&digest(payload));
    let replacement = record(&digest("opaque-replacement"));
    let same_payload_replacement = record(&digest(payload));
    let store = PendingDeliveryPayloadStore::open(temp.path(), &HashSet::new()).unwrap();
    store.write(&first, payload).unwrap();
    store.write(&second, payload).unwrap();
    let live = HashSet::from([second.content_digest.clone()]);
    store.write(&same_payload_replacement, payload).unwrap();
    store
        .retire_replaced(Some(&first), &same_payload_replacement, &live)
        .unwrap();
    assert!(!store.path_for_test(&first).exists());
    assert_eq!(store.resolve(&second).unwrap(), payload);
    store.write(&replacement, "opaque-replacement").unwrap();
    store
        .retire_replaced(Some(&same_payload_replacement), &replacement, &live)
        .unwrap();
    assert!(!store.path_for_test(&same_payload_replacement).exists());
    assert_eq!(store.resolve(&replacement).unwrap(), "opaque-replacement");
    assert_eq!(store.resolve(&second).unwrap(), payload);
}

#[test]
fn sweep_keeps_both_live_identical_payload_records() {
    let temp = tempfile::tempdir().unwrap();
    let payload = "opaque-sweep-body";
    let first = record(&digest(payload));
    let second = record(&digest(payload));
    let orphan = record(&digest(payload));
    let store = PendingDeliveryPayloadStore::open(temp.path(), &HashSet::new()).unwrap();
    for owner in [&first, &second, &orphan] {
        store.write(owner, payload).unwrap();
    }
    let (mut state, tab) = crate::multiplexer::state::MultiplexerState::new();
    let sibling = state.create_tab_record();
    state.get_tab_mut(&tab).unwrap().pending_delivery = Some(first.clone());
    state.get_tab_mut(&sibling).unwrap().pending_delivery = Some(second.clone());
    let live = live_payload_keys(&state.snapshot());
    let reopened = PendingDeliveryPayloadStore::open(temp.path(), &live).unwrap();
    assert_eq!(reopened.resolve(&first).unwrap(), payload);
    assert_eq!(reopened.resolve(&second).unwrap(), payload);
    assert!(!store.path_for_test(&orphan).exists());
}

#[test]
fn upgrade_digest_payload_survives_sibling_terminal_and_replace_until_orphaned() {
    let temp = tempfile::tempdir().unwrap();
    let payload = "opaque-upgrade-body";
    let first = record(&digest(payload));
    let second = record(&digest(payload));
    let store = PendingDeliveryPayloadStore::open(temp.path(), &HashSet::new()).unwrap();
    let old_path = store.dir.join(&first.content_digest);
    fs::write(&old_path, payload).unwrap();
    let (mut state, tab) = crate::multiplexer::state::MultiplexerState::new();
    state.get_tab_mut(&tab).unwrap().pending_delivery = Some(second.clone());
    let reopened =
        PendingDeliveryPayloadStore::open(temp.path(), &live_payload_keys(&state.snapshot()))
            .unwrap();
    let mut terminal = Some(first.clone());
    terminal.as_mut().unwrap().confirm().unwrap();
    reopened
        .free_terminal_pair(
            &mut terminal,
            "confirmed",
            &HashSet::from([second.content_digest.clone()]),
        )
        .unwrap();
    assert_eq!(reopened.resolve(&second).unwrap(), payload);
    let replacement = record(&digest("opaque-new-body"));
    reopened.write(&replacement, "opaque-new-body").unwrap();
    reopened
        .retire_replaced(
            Some(&first),
            &replacement,
            &live_payload_keys(&state.snapshot()),
        )
        .unwrap();
    assert_eq!(reopened.resolve(&second).unwrap(), payload);
    reopened
        .sweep_orphans(&HashSet::from([payload_key(&replacement)]))
        .unwrap();
    assert!(!old_path.exists());
    assert_eq!(reopened.resolve(&replacement).unwrap(), "opaque-new-body");
}

#[test]
fn corrupt_owned_payload_does_not_fall_back_to_valid_upgrade_payload() {
    let temp = tempfile::tempdir().unwrap();
    let payload = "opaque-integrity-body";
    let owner = record(&digest(payload));
    let store = PendingDeliveryPayloadStore::open(temp.path(), &HashSet::new()).unwrap();
    fs::write(store.dir.join(&owner.content_digest), payload).unwrap();
    store.write(&owner, payload).unwrap();
    fs::write(store.path_for_test(&owner), "opaque-corruption").unwrap();
    assert_eq!(
        store.resolve(&owner),
        Err(PayloadStoreError::DigestMismatch {
            digest: owner.content_digest
        })
    );
}

mod replacement;
