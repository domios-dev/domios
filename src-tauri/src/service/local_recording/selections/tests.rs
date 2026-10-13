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
    let path = dir.path().join("blocked");
    let store = RecordingSelectionStore::open(path.clone());
    std::fs::create_dir(&path).unwrap();
    assert!(store.set("own", &devices("input")).is_err());
    assert_eq!(store.get("own").unwrap(), None);
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
