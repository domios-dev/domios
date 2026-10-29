use super::*;
use crate::session::store::StorageDeskAdapter;

fn close(authority: StorageAuthority) {
    shutdown_bounded(&authority).expect("shutdown");
}

#[test]
fn startup_uses_only_state_db_and_preserves_legacy_files() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("sessions/last.json");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    let original = br#"{"workspaces":[{"id":"obsolete"}]}"#;
    std::fs::write(&source, original).unwrap();
    std::fs::write(root.path().join("crons.json"), b"{").unwrap();
    for _ in 0..2 {
        let authority = open_instance_storage_at(root.path());
        let storage = authority.storage().expect("current storage is ready");
        assert!(StorageDeskAdapter::from_storage(storage.clone()).snapshot().unwrap().workspaces.is_empty());
        assert!(storage.legacy_import_marker().wait(STARTUP_DEADLINE).unwrap().value.is_none());
        assert_eq!(std::fs::read(&source).unwrap(), original);
        assert_eq!(std::fs::read(root.path().join("crons.json")).unwrap(), b"{");
        close(authority);
    }
}

#[test]
fn startup_refuses_a_second_owner_for_the_same_root() {
    let root = tempfile::tempdir().unwrap();
    let first = open_instance_storage_at(root.path());
    assert!(first.storage().is_some());
    assert!(Storage::open(root.path(), StorageConfig::default()).is_err());
    close(first);
}

#[test]
fn unavailable_authority_returns_a_typed_error() {
    let authority = StorageAuthority::Unavailable { storage: None, reason: "database failed".into() };
    let error = authority.require_ready().err().unwrap();
    assert_eq!(error, StorageUnavailableError);
    assert_eq!(error.to_string(), STORAGE_UNAVAILABLE_MESSAGE);
}

#[test]
fn current_db_restores_workspace_cwd_without_reading_stale_json() {
    let root = tempfile::tempdir().unwrap();
    let first = open_instance_storage_at(root.path());
    let adapter = StorageDeskAdapter::from_storage(first.storage().unwrap().clone());
    adapter.import_legacy(r#"{"version":"1","timestamp":1,"active_workspace_id":"ws-current","teams":[],"workspaces":[{"id":"ws-current","name":"Current project","tabs":[],"active_tab_id":null,"cwd":"database-cwd","provisioned_cwd":null}]}"#, None).unwrap();
    drop(adapter);
    close(first);
    let source = root.path().join("sessions/last.json");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, br#"{"workspaces":[{"cwd":"stale-json-cwd"}]}"#).unwrap();
    let reopened = open_instance_storage_at(root.path());
    let snapshot = StorageDeskAdapter::from_storage(reopened.storage().unwrap().clone()).snapshot().unwrap();
    assert_eq!(snapshot.workspaces[0].cwd.as_deref(), Some("database-cwd"));
    assert_eq!(snapshot.workspaces[0].name, "Current project");
    close(reopened);
}
