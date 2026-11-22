use super::*;

fn devices(id: &str) -> SelectedDevices {
    SelectedDevices {
        input_id: id.into(),
        collection_id: "collection".into(),
        microphone_id: "microphone".into(),
    }
}

#[test]
fn recording_selection_round_trips_per_plugin_without_exposing_foreign_selection() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("selections.json");
    let store = RecordingSelectionStore::open(path.clone());
    store.set("own", &devices("input")).unwrap();
    store.set("other", &devices("other-input")).unwrap();
    let reopened = RecordingSelectionStore::open(path);
    assert_eq!(reopened.get("own").unwrap(), Some(devices("input")));
    assert_eq!(reopened.get("other").unwrap(), Some(devices("other-input")));
    assert_eq!(reopened.get("missing").unwrap(), None);
}

#[test]
fn recording_selection_failed_save_does_not_change_memory() {
    let dir = tempfile::tempdir().unwrap();
    // A file where the parent folder should be fails every platform's create and rename.
    let blocker = dir.path().join("not-a-folder");
    std::fs::write(&blocker, b"").unwrap();
    let store = RecordingSelectionStore::open(blocker.join("selections.json"));
    assert!(store.set("own", &devices("input")).is_err());
    // An unreadable store may refuse the read, but it must never report the unsaved choice.
    assert!(!matches!(store.get("own"), Ok(Some(_))));
}

#[test]
fn recording_selection_invalid_identity_is_never_saved() {
    let dir = tempfile::tempdir().unwrap();
    let store = RecordingSelectionStore::open(dir.path().join("selections.json"));
    for id in [String::new(), "x".repeat(4097), "input\0suffix".into()] {
        assert!(store.set("own", &devices(&id)).is_err());
    }
    for microphone in ["default", "communications"] {
        let mut selection = devices("input");
        selection.microphone_id = microphone.into();
        assert!(store.set("own", &selection).is_err());
    }
    assert_eq!(store.get("own").unwrap(), None);
}

#[test]
fn recording_selection_corrupt_store_refuses_read_and_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("selections.json");
    std::fs::write(&path, "{broken").unwrap();
    let store = RecordingSelectionStore::open(path);
    assert_eq!(
        store.get("own").unwrap_err(),
        "recording_selection_store_unavailable"
    );
    assert_eq!(
        store.set("own", &devices("input")).unwrap_err(),
        "recording_selection_store_unavailable"
    );
}

#[test]
fn recording_enabled_preference_round_trips_per_plugin() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("local-recording-selections.json");
    let store = RecordingSelectionStore::open(path.clone());
    assert!(!store.enabled("own").unwrap());
    store.set_enabled("own", true).unwrap();
    store.set_enabled("other", false).unwrap();

    let reopened = RecordingSelectionStore::open(path);
    assert!(reopened.enabled("own").unwrap());
    assert!(!reopened.enabled("other").unwrap());
    assert!(!reopened.enabled("missing").unwrap());
    reopened.set_enabled("own", false).unwrap();
    assert!(!reopened.enabled("own").unwrap());
}

#[test]
fn recording_enabled_failed_save_does_not_change_memory() {
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("not-a-folder");
    std::fs::write(&blocker, b"").unwrap();
    let store = RecordingSelectionStore::open(blocker.join("selections.json"));
    assert!(store.set_enabled("own", true).is_err());
    assert!(!matches!(store.enabled("own"), Ok(true)));
}

#[test]
fn recording_enabled_corrupt_store_refuses_read_and_write() {
    let dir = tempfile::tempdir().unwrap();
    let selections_path = dir.path().join("local-recording-selections.json");
    let enabled_path = dir.path().join("local-recording-enabled.json");
    std::fs::write(&enabled_path, "{broken").unwrap();
    let store = RecordingSelectionStore::open(selections_path);
    assert_eq!(
        store.enabled("own").unwrap_err(),
        "recording_preference_store_unavailable"
    );
    assert_eq!(
        store.set_enabled("own", true).unwrap_err(),
        "recording_preference_store_unavailable"
    );
}
