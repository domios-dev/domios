use super::*;

const V1: &str = include_str!("../../../../tests/fixtures/releases/v1.18.6.sql");
const V4: &str = include_str!("../../../../tests/fixtures/releases/forward-v4.sql");
const V5: &str = include_str!("../../../../tests/fixtures/releases/forward-v5.sql");

#[test]
fn every_pending_source_has_one_readable_backup_of_its_original_records() {
    for fixture in [
        include_str!("../../../../tests/fixtures/releases/supported-v0.sql"),
        V1,
        include_str!("../../../../tests/fixtures/releases/v1.18.7.sql"),
        include_str!("../../../../tests/fixtures/releases/forward-v2.sql"),
        include_str!("../../../../tests/fixtures/releases/forward-v3.sql"),
        V4,
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("plans.db");
        let mut connection = Connection::open(&path).unwrap();
        connection.execute_batch(fixture).unwrap();
        let before = snapshot(&connection);
        let legacy = version(&connection).unwrap() == 0;
        initialize(&mut connection, &path).unwrap();
        let backup = if legacy {
            directory.path().join("plans.db.pre-1.18")
        } else {
            backup::path(&path)
        };
        let saved =
            Connection::open_with_flags(&backup, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        assert_eq!(snapshot(&saved), before);
        assert!(validate_source(&saved).is_ok());
        store_db::release_upgrade_tests::assert_input_records(&saved, legacy);
        assert_eq!(
            std::fs::read_dir(directory.path()).unwrap().count(),
            2,
            "only the database and its one backup remain"
        );
        let bytes = std::fs::read(&backup).unwrap();
        initialize(&mut connection, &path).unwrap();
        assert_eq!(std::fs::read(&backup).unwrap(), bytes);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 2);
    }
}

#[test]
fn a_later_upgrade_atomically_replaces_the_single_backup() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("plans.db");
    let backup = backup::path(&path);
    let mut previous = None;
    for fixture in [V1, V4] {
        if path.exists() {
            std::fs::remove_file(&path).unwrap();
        }
        let mut connection = Connection::open(&path).unwrap();
        connection.execute_batch(fixture).unwrap();
        let before = snapshot(&connection);
        initialize(&mut connection, &path).unwrap();
        assert_eq!(snapshot(&Connection::open(&backup).unwrap()), before);
        let bytes = std::fs::read(&backup).unwrap();
        if let Some(previous) = previous {
            assert_ne!(bytes, previous);
        }
        previous = Some(bytes);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 2);
    }
}

#[test]
fn backup_failure_refuses_before_any_source_write_and_cleans_staging() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("plans.db");
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch(V4).unwrap();
    let before = snapshot(&connection);
    drop(connection);
    let bytes = std::fs::read(&path).unwrap();
    let backup = backup::path(&path);
    std::fs::create_dir(&backup).unwrap();
    let error = PlanDatabase::open(path.clone()).err().unwrap().to_string();
    assert!(
        error.contains("plan migration refused: backup to"),
        "{error}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(snapshot(&Connection::open(&path).unwrap()), before);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 2);
}

#[test]
fn fresh_empty_and_current_databases_do_not_write_a_backup() {
    for fixture in [None, Some(""), Some(V5)] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("plans.db");
        if let Some(fixture) = fixture {
            Connection::open(&path)
                .unwrap()
                .execute_batch(fixture)
                .unwrap();
        }
        drop(PlanDatabase::open(path.clone()).unwrap());
        assert!(!backup::path(&path).exists());
        assert!(!directory.path().join("plans.db.pre-1.18").exists());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}

#[test]
fn online_backup_includes_committed_wal_records_before_migration() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("plans.db");
    let writer = Connection::open(&path).unwrap();
    writer
        .execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
        .unwrap();
    writer.execute_batch(V4).unwrap();
    writer
        .execute("UPDATE plans SET title = 'committed in WAL'", [])
        .unwrap();
    let before = snapshot(&writer);
    assert!(directory.path().join("plans.db-wal").exists());
    drop(PlanDatabase::open(path.clone()).unwrap());
    let saved = Connection::open(backup::path(&path)).unwrap();
    assert_eq!(snapshot(&saved), before);
    assert_eq!(version(&saved).unwrap(), 4);
    assert!(validate_source(&saved).is_ok());
}
