use super::*;
use crate::service_plan::store_db::PlanDatabase;
use rusqlite::{types::Value as SqlValue, OpenFlags};

#[derive(Debug, PartialEq)]
struct Snapshot {
    schema: Vec<(String, String, String, Option<String>)>,
    rows: Vec<(String, Vec<Vec<SqlValue>>)>,
    version: i64,
    journal: String,
    files: Vec<Option<Vec<u8>>>,
}

fn snapshot(path: &Path) -> Snapshot {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let schema = store_db::admission::schema(&connection).unwrap();
    let rows = schema
        .iter()
        .filter(|(kind, ..)| kind == "table")
        .map(|(_, name, ..)| {
            let sql = format!("SELECT * FROM \"{}\"", name.replace('"', "\"\""));
            let mut statement = connection.prepare(&sql).unwrap();
            let count = statement.column_count();
            let rows = statement
                .query_map([], |row| {
                    (0..count)
                        .map(|column| row.get::<_, SqlValue>(column))
                        .collect::<rusqlite::Result<Vec<_>>>()
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            (name.clone(), rows)
        })
        .collect();
    let version = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    let journal = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    drop(connection);
    let files = ["", "-wal", "-shm", "-journal"]
        .into_iter()
        .map(|suffix| {
            let mut file = path.as_os_str().to_owned();
            file.push(suffix);
            match std::fs::read(&file) {
                Ok(bytes) => Some(bytes),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => panic!("snapshot {}: {error}", Path::new(&file).display()),
            }
        })
        .collect();
    Snapshot {
        schema,
        rows,
        version,
        journal,
        files,
    }
}

fn refuses_without_mutation(path: &Path) {
    let before = snapshot(path);
    let error = PlanDatabase::open(path.to_path_buf())
        .err()
        .expect("schema must be refused");
    assert!(matches!(error, PlanError::Io(_)), "{error}");
    assert_eq!(snapshot(path), before);
    assert!(!backup_path(path).exists());
}

#[test]
fn future_version_is_refused_without_mutation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("plans.db");
    let connection = Connection::open(&path).unwrap();
    store_db::create_schema(&connection).unwrap();
    connection.execute_batch("INSERT INTO plan_operations(entity_id, kind, state, submitted_at) VALUES ('retained', 'fixture', 'queued', 1)").unwrap();
    connection
        .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
        .unwrap();
    drop(connection);
    refuses_without_mutation(&path);
}

#[test]
fn unknown_nonempty_v0_is_refused_without_mutation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("plans.db");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE unknown(payload TEXT); INSERT INTO unknown VALUES ('retained');",
        )
        .unwrap();
    drop(connection);
    refuses_without_mutation(&path);
}

#[test]
fn current_schema_with_unknown_table_is_refused_without_mutation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("plans.db");
    let connection = Connection::open(&path).unwrap();
    store_db::create_schema(&connection).unwrap();
    connection
        .pragma_update(None, "user_version", SCHEMA_VERSION)
        .unwrap();
    connection.execute_batch("CREATE TABLE sqliteXunknown(payload TEXT); INSERT INTO sqliteXunknown VALUES ('retained')").unwrap();
    drop(connection);
    refuses_without_mutation(&path);
}

#[test]
fn missing_legacy_child_column_is_structural_and_refused_without_mutation() {
    let directory = tempfile::tempdir().unwrap();
    let path = super::tests::legacy_database(
        directory.path(),
        &[super::tests::legacy_plan("pl_retained")],
    );
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch("ALTER TABLE plan_task_milestones RENAME COLUMN document_json TO payload")
        .unwrap();
    assert!(
        read_legacy_plans(&connection).is_err(),
        "SQL failure is not document quarantine"
    );
    drop(connection);
    refuses_without_mutation(&path);
}

#[test]
fn mixed_legacy_and_plan_level_tables_are_refused_without_mutation() {
    let directory = tempfile::tempdir().unwrap();
    let path = super::tests::legacy_database(
        directory.path(),
        &[super::tests::legacy_plan("pl_retained")],
    );
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(super::super::store_sync::PLAN_LEVEL_SCHEMA)
        .unwrap();
    drop(connection);
    refuses_without_mutation(&path);
}

#[test]
fn partial_current_schema_is_refused_without_mutation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("plans.db");
    let connection = Connection::open(&path).unwrap();
    store_db::create_schema(&connection).unwrap();
    connection
        .pragma_update(None, "user_version", SCHEMA_VERSION)
        .unwrap();
    connection
        .execute_batch("DROP TABLE plan_heartbeat")
        .unwrap();
    drop(connection);
    refuses_without_mutation(&path);
}

fn legacy_database_with_imports(directory: &Path, extra: &str) -> PathBuf {
    let path = super::tests::legacy_database(directory, &[super::tests::legacy_plan("pl_imports")]);
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch(extra).unwrap();
    connection
        .execute(
            "INSERT INTO legacy_imports(root, imported_at) VALUES ('/retained/root', 1)",
            [],
        )
        .unwrap();
    path
}

fn table_count(path: &Path, name: &str) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            [name],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn legacy_imports_table_is_admitted_backed_up_and_dropped() {
    let directory = tempfile::tempdir().unwrap();
    let path = legacy_database_with_imports(directory.path(), LEGACY_IMPORTS_SCHEMA);

    drop(PlanDatabase::open(path.clone()).unwrap());
    let backup = backup_path(&path);
    assert_eq!(table_count(&path, "legacy_imports"), 0);
    assert_eq!(table_count(&backup, "legacy_imports"), 1);
    let backup_bytes = std::fs::read(&backup).unwrap();

    let reopened = PlanDatabase::open(path.clone()).unwrap();
    assert_eq!(reopened.load_plans_for_test().len(), 1);
    assert_eq!(std::fs::read(&backup).unwrap(), backup_bytes);
}

#[test]
fn altered_legacy_imports_table_is_refused_without_mutation() {
    let directory = tempfile::tempdir().unwrap();
    let path = legacy_database_with_imports(
        directory.path(),
        "CREATE TABLE legacy_imports(root TEXT PRIMARY KEY, imported_at INTEGER NOT NULL, origin TEXT);",
    );
    refuses_without_mutation(&path);
}

#[test]
fn legacy_imports_with_unknown_table_is_refused_without_mutation() {
    let directory = tempfile::tempdir().unwrap();
    let path = legacy_database_with_imports(directory.path(), LEGACY_IMPORTS_SCHEMA);
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TABLE unknown(payload TEXT); INSERT INTO unknown VALUES ('retained');",
        )
        .unwrap();
    refuses_without_mutation(&path);
}
