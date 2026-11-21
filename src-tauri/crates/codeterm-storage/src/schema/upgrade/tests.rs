use super::*;
use crate::schema::{MIGRATIONS, SCHEMA_VERSION};
use crate::{Storage, StorageConfig};
use std::path::{Path, PathBuf};
use std::time::Duration;

const PREVIOUS_RELEASE_SCHEMA: i32 = 28;

fn seed(root: &Path, version: i32) -> Layout {
    let layout = Layout::of_root(root);
    layout.create_dirs(true).unwrap();
    assets::create_owner_file(&layout.database).unwrap();
    let mut conn = open_existing(&layout.database, OpenFlags::SQLITE_OPEN_READ_WRITE).unwrap();
    let tx = conn.transaction().unwrap();
    for step in &MIGRATIONS[..version as usize] {
        tx.execute_batch(step).unwrap();
    }
    tx.pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    tx.pragma_update(None, "user_version", version).unwrap();
    tx.execute_batch(
        "INSERT INTO workspaces(id, ordinal, name, protected, task_key_next, revision)
         VALUES ('retained-workspace', 0, 'Retained', 0, 7, 73);",
    )
    .unwrap();
    tx.commit().unwrap();
    conn.close().unwrap();
    layout
}

fn backups(layout: &Layout) -> Vec<PathBuf> {
    std::fs::read_dir(&layout.dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("pre-upgrade-")
        })
        .collect()
}

fn assert_retained(conn: &Connection, name: &str) {
    let row: (String, i64, i64) = conn
        .query_row(
            "SELECT name, task_key_next, revision FROM workspaces WHERE id='retained-workspace'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(row, (name.to_string(), 7, 73));
}

#[test]
fn previous_release_upgrades_with_wal_data_and_a_durable_pre_change_image() {
    let root = tempfile::tempdir().unwrap();
    let layout = seed(root.path(), PREVIOUS_RELEASE_SCHEMA);
    let source = open_existing(&layout.database, OpenFlags::SQLITE_OPEN_READ_WRITE).unwrap();
    source.pragma_update(None, "journal_mode", "WAL").unwrap();
    source
        .execute("UPDATE workspaces SET name='Committed in WAL'", [])
        .unwrap();

    let storage = Storage::open(root.path(), StorageConfig::default()).unwrap();
    let conn = open_existing(&layout.database, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(
        pragma_value::<i32>(&conn, "user_version").unwrap(),
        SCHEMA_VERSION
    );
    assert_retained(&conn, "Committed in WAL");
    conn.prepare("SELECT provider, owner_instance_id FROM cron_runs")
        .unwrap();
    conn.prepare("SELECT provider, owner_instance_id FROM cron_last_run_mirrors")
        .unwrap();
    let images = backups(&layout);
    assert_eq!(images.len(), 1);
    let image = open_existing(&images[0], OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(
        pragma_value::<i32>(&image, "user_version").unwrap(),
        PREVIOUS_RELEASE_SCHEMA
    );
    assert_retained(&image, "Committed in WAL");
    assert!(image.prepare("SELECT provider FROM cron_runs").is_err());
    assert_eq!(
        pragma_value::<String>(&image, "journal_mode").unwrap(),
        "delete"
    );
    drop(source);
    drop(conn);
    storage.shutdown().wait(Duration::from_secs(5)).unwrap();
}

#[test]
fn every_schema_prefix_upgrades_through_all_later_steps() {
    for version in 1..SCHEMA_VERSION {
        let root = tempfile::tempdir().unwrap();
        let layout = seed(root.path(), version);
        let storage = Storage::open(root.path(), StorageConfig::default())
            .unwrap_or_else(|error| panic!("schema {version} failed to upgrade: {error}"));
        let conn = open_existing(&layout.database, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        assert_eq!(
            pragma_value::<i32>(&conn, "user_version").unwrap(),
            SCHEMA_VERSION
        );
        assert_retained(&conn, "Retained");
        drop(conn);
        storage.shutdown().wait(Duration::from_secs(5)).unwrap();
    }
}

#[test]
fn foreign_and_newer_databases_are_refused_before_backup_or_changes() {
    for (application_id, version) in [
        (0x1234, PREVIOUS_RELEASE_SCHEMA),
        (APPLICATION_ID, SCHEMA_VERSION + 1),
    ] {
        let root = tempfile::tempdir().unwrap();
        let layout = seed(root.path(), PREVIOUS_RELEASE_SCHEMA);
        let conn = open_existing(&layout.database, OpenFlags::SQLITE_OPEN_READ_WRITE).unwrap();
        conn.pragma_update(None, "application_id", application_id)
            .unwrap();
        conn.pragma_update(None, "user_version", version).unwrap();
        conn.close().unwrap();
        let before = std::fs::read(&layout.database).unwrap();
        let error = Storage::open(root.path(), StorageConfig::default())
            .err()
            .unwrap();
        assert!(matches!(error, StorageError::Incompatible { .. }));
        assert_eq!(std::fs::read(&layout.database).unwrap(), before);
        assert!(backups(&layout).is_empty());
    }
}

#[test]
fn failed_upgrade_rolls_back_all_steps_and_version_and_keeps_the_backup() {
    let root = tempfile::tempdir().unwrap();
    let layout = seed(root.path(), PREVIOUS_RELEASE_SCHEMA);
    let conn = open_existing(&layout.database, OpenFlags::SQLITE_OPEN_READ_WRITE).unwrap();
    conn.execute_batch("ALTER TABLE cron_last_run_mirrors ADD COLUMN provider TEXT;")
        .unwrap();
    conn.close().unwrap();
    assert!(Storage::open(root.path(), StorageConfig::default()).is_err());
    let conn = open_existing(&layout.database, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(
        pragma_value::<i32>(&conn, "user_version").unwrap(),
        PREVIOUS_RELEASE_SCHEMA
    );
    assert!(conn.prepare("SELECT provider FROM cron_runs").is_err());
    assert_retained(&conn, "Retained");
    assert_eq!(backups(&layout).len(), 1);
}
