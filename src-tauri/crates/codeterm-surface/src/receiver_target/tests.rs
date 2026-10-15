use super::*;
const RECEIVER: &str = "receiver";

fn store_at(dir: &tempfile::TempDir) -> ReceiverTargetStore {
    ReceiverTargetStore::open(dir.path().join("receiver-targets.json"))
}

#[test]
fn pebble_receiver_target_store_set_clear_get() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_at(&dir);
    assert_eq!(store.get(RECEIVER), None);

    assert!(store.set(RECEIVER, "tab-a").unwrap());
    assert!(
        !store.set(RECEIVER, "tab-a").unwrap(),
        "rebinding the same tab is not a change"
    );
    assert_eq!(store.get(RECEIVER).as_deref(), Some("tab-a"));

    assert!(store.set(RECEIVER, "tab-b").unwrap());
    assert_eq!(store.get(RECEIVER).as_deref(), Some("tab-b"));
    assert_eq!(store.bindings().len(), 1, "one tab per receiver");

    assert!(store.clear(RECEIVER).unwrap());
    assert!(!store.clear(RECEIVER).unwrap());
    assert_eq!(store.get(RECEIVER), None);
}

#[test]
fn pebble_receiver_target_store_round_trips_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = store_at(&dir);
        store.set(RECEIVER, "tab-a").unwrap();
        store.set("second-receiver", "tab-b").unwrap();
    }
    let reopened = store_at(&dir);
    assert_eq!(reopened.get(RECEIVER).as_deref(), Some("tab-a"));
    assert_eq!(reopened.get("second-receiver").as_deref(), Some("tab-b"));

    reopened.clear(RECEIVER).unwrap();
    drop(reopened);
    let after_reset = store_at(&dir);
    assert_eq!(after_reset.get(RECEIVER), None);
    assert_eq!(after_reset.get("second-receiver").as_deref(), Some("tab-b"));
}

#[test]
fn pebble_receiver_target_store_forget_tab_clears_only_that_tab() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_at(&dir);
    store.set("a", "tab-1").unwrap();
    store.set("b", "tab-1").unwrap();
    store.set("c", "tab-2").unwrap();

    assert!(store.forget_tab("tab-1").unwrap());
    assert!(!store.forget_tab("tab-1").unwrap());
    assert_eq!(store.get("a"), None);
    assert_eq!(store.get("b"), None);
    assert_eq!(store.get("c").as_deref(), Some("tab-2"));
    assert_eq!(store_at(&dir).bindings().len(), 1);
}

#[test]
fn pebble_receiver_target_store_unreadable_file_opens_empty_and_is_never_saved_over() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receiver-targets.json");
    std::fs::write(&path, "{").unwrap();
    let store = store_at(&dir);
    assert!(store.bindings().is_empty());
    assert!(store.set(RECEIVER, "tab-a").is_err());
    assert_eq!(store.get(RECEIVER), None);
    assert!(!path.exists());
    let copies = state_file(&path).corrupt_copies();
    assert_eq!(std::fs::read_to_string(&copies[0]).unwrap(), "{");
}

#[test]
fn pebble_receiver_target_store_recovers_bindings_from_backup() {
    let dir = tempfile::tempdir().unwrap();
    store_at(&dir).set(RECEIVER, "tab-a").unwrap();
    std::fs::write(dir.path().join("receiver-targets.json"), "{\"rec").unwrap();
    assert_eq!(store_at(&dir).get(RECEIVER).as_deref(), Some("tab-a"));
}

#[test]
fn pebble_receiver_target_failed_write_keeps_the_previous_binding() {
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("blocker");
    std::fs::write(&blocker, "a file, not a directory").unwrap();
    let store = ReceiverTargetStore::open(blocker.join("receiver-targets.json"));
    assert!(store.set(RECEIVER, "tab-a").is_err());
    assert_eq!(
        store.get(RECEIVER),
        None,
        "memory never claims a binding a restart would lose"
    );
}

#[test]
fn pebble_receiver_target_routes_only_to_a_live_bound_tab() {
    assert_eq!(
        live_target(None, |_| unreachable!("an unbound receiver checks no tab")),
        None
    );
    for (liveness, expected) in [
        (TargetLiveness::Live, Some("tab-a".to_string())),
        (TargetLiveness::NotLive, None),
        (TargetLiveness::Absent, None),
    ] {
        let routed = live_target(Some("tab-a".into()), |tab_id| {
            assert_eq!(tab_id, "tab-a");
            liveness
        });
        assert_eq!(routed, expected, "{liveness:?}");
    }
}

#[test]
fn pebble_receiver_target_host_json_carries_codes_not_details() {
    let view: serde_json::Value = serde_json::from_str(&host_json(Ok(ReceiverTargetView {
        plugin_id: RECEIVER.into(),
        tab_id: Some("tab-a".into()),
        live: true,
    })))
    .unwrap();
    assert_eq!(
        view,
        serde_json::json!({ "pluginId": RECEIVER, "tabId": "tab-a", "live": true })
    );

    let refused: serde_json::Value =
        serde_json::from_str(&host_json(Err(ReceiverTargetError::PersistFailed))).unwrap();
    assert_eq!(refused["error"], "persist_failed");
    assert_eq!(refused.as_object().unwrap().len(), 2);
}
