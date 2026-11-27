use super::*;
use crate::session::store::{SaveIntent, StorageDeskAdapter, WorkspaceSnapshot};

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
        assert!(StorageDeskAdapter::from_storage(storage.clone())
            .snapshot()
            .unwrap()
            .workspaces
            .is_empty());
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
    let authority = StorageAuthority::Unavailable {
        storage: None,
        reason: "database failed".into(),
    };
    let error = authority.require_ready().err().unwrap();
    assert_eq!(error, StorageUnavailableError);
    assert_eq!(error.to_string(), STORAGE_UNAVAILABLE_MESSAGE);
}

#[test]
fn current_db_restores_workspace_cwd_without_reading_stale_json() {
    let root = tempfile::tempdir().unwrap();
    let first = open_instance_storage_at(root.path());
    let adapter = StorageDeskAdapter::from_storage(first.storage().unwrap().clone());
    let state = adapter.state().unwrap();
    let mut snapshot = adapter.snapshot().unwrap();
    snapshot.timestamp = 1;
    snapshot.active_workspace_id = Some("ws-current".into());
    snapshot.workspaces.push(WorkspaceSnapshot {
        id: "ws-current".into(),
        name: "Current project".into(),
        tabs: Vec::new(),
        active_tab_id: None,
        cwd: Some("database-cwd".into()),
        provisioned_cwd: None,
        protected: false,
        created_at_ms: 0,
        sort_order: 0,
        task_keys: Default::default(),
    });
    adapter
        .delta_snapshot(
            &snapshot,
            state.snapshot.agent_session_identities,
            state.revision,
            SaveIntent::PreserveDesk,
        )
        .unwrap()
        .wait(STARTUP_DEADLINE)
        .unwrap();
    drop(adapter);
    close(first);
    let source = root.path().join("sessions/last.json");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, br#"{"workspaces":[{"cwd":"stale-json-cwd"}]}"#).unwrap();
    let (reopened, snapshot) = restore_storage_at(root.path());
    let snapshot = snapshot.unwrap();
    assert_eq!(snapshot.workspaces[0].cwd.as_deref(), Some("database-cwd"));
    assert_eq!(snapshot.workspaces[0].name, "Current project");
    close(reopened);
}

#[test]
fn startup_restore_withholds_storage_authority_on_real_database_corruption() {
    use std::io::{Seek, SeekFrom, Write};

    let root = tempfile::tempdir().unwrap();
    let (authority, snapshot) = restore_storage_at(root.path());
    assert!(authority.require_ready().is_ok());
    assert!(snapshot.unwrap().workspaces.is_empty());
    close(authority);

    let path = root.path().join("storage/state.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(
        "CREATE TABLE corruption_fixture(value);
         INSERT INTO corruption_fixture VALUES ('persisted');",
    )
    .unwrap();
    let page_size: u64 = db
        .query_row("PRAGMA page_size", [], |row| row.get(0))
        .unwrap();
    let root_page: u64 = db
        .query_row(
            "SELECT rootpage FROM sqlite_schema WHERE name = 'corruption_fixture'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    db.close().unwrap();

    let original = std::fs::read(&path).unwrap();
    assert!(root_page > 1);
    let mut db = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    db.seek(SeekFrom::Start((root_page - 1) * page_size))
        .unwrap();
    db.write_all(&[0]).unwrap();
    drop(db);
    let corrupt = std::fs::read(&path).unwrap();
    assert_eq!(corrupt.len(), original.len());
    assert_ne!(corrupt, original);

    let (authority, snapshot) = restore_storage_at(root.path());
    assert_eq!(
        authority.require_ready().err(),
        Some(StorageUnavailableError)
    );
    assert!(snapshot.is_none());
    assert!(authority
        .unavailable_reason()
        .unwrap()
        .starts_with("storage open failed:"));
    assert_eq!(std::fs::read(&path).unwrap(), corrupt);
}
