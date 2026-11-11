use crate::schema::{validate_identity, APPLICATION_ID, SCHEMA_VERSION};
use crate::StorageError;
use rusqlite::Connection;

#[test]
fn startup_accepts_only_the_1_17_schema_without_upgrading() {
    let conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    for version in -1..=SCHEMA_VERSION + 1 {
        conn.pragma_update(None, "user_version", version).unwrap();
        let actual: i32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        if actual == SCHEMA_VERSION {
            assert_eq!(validate_identity(&conn).unwrap(), SCHEMA_VERSION);
        } else {
            assert!(matches!(
                validate_identity(&conn),
                Err(StorageError::Incompatible { .. })
            ));
        }
        let after: i32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(after, actual);
    }
}

#[test]
fn schema_v1_store_is_refused_without_changing_its_records() {
    let root = tempfile::tempdir().unwrap();
    let database = crate::assets::Layout::of_root(root.path()).database;
    std::fs::create_dir_all(database.parent().unwrap()).unwrap();
    let conn = Connection::open(&database).unwrap();
    conn.execute_batch(crate::schema::MIGRATIONS[0]).unwrap();
    conn.pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    conn.pragma_update(None, "user_version", 1).unwrap();
    conn.execute_batch(
        "INSERT INTO workspaces(id, ordinal, name, protected, task_key_next, revision)
         VALUES ('retained-workspace', 0, 'Retained', 0, 7, 73);",
    )
    .unwrap();
    conn.close().unwrap();
    let before = std::fs::read(&database).unwrap();

    let error = crate::Storage::open(root.path(), crate::StorageConfig::default())
        .err()
        .expect("schema v1 is unsupported");
    assert!(matches!(
        error,
        StorageError::Incompatible {
            application_id: APPLICATION_ID,
            schema_version: 1,
            supported: SCHEMA_VERSION,
        }
    ));
    assert_eq!(
        error.to_string(),
        format!("incompatible database {APPLICATION_ID:#x} schema 1/{SCHEMA_VERSION}")
    );
    assert_eq!(std::fs::read(&database).unwrap(), before);
    let conn =
        Connection::open_with_flags(&database, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let row: (String, i64, i64) = conn
        .query_row(
            "SELECT id, task_key_next, revision FROM workspaces",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(row, ("retained-workspace".into(), 7, 73));
}
