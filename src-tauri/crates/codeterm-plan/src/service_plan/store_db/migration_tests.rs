use super::*;
use crate::service_plan::store_db::{self, PlanDatabase};

#[path = "migration_tests/backup.rs"]
mod backup_tests;

type Schema = Vec<(String, String, String, Option<String>)>;
type Rows = Vec<(String, Vec<Vec<rusqlite::types::Value>>)>;

fn snapshot(connection: &Connection) -> (i64, i64, Schema, Rows) {
    let schema = admission::schema(connection).unwrap();
    let rows = schema
        .iter()
        .filter(|entry| entry.0 == "table")
        .map(|entry| {
            let mut statement = connection
                .prepare(&format!("SELECT * FROM \"{}\"", entry.1))
                .unwrap();
            let columns = statement.column_count();
            let rows = statement
                .query_map([], |row| {
                    (0..columns)
                        .map(|column| row.get(column))
                        .collect::<rusqlite::Result<Vec<_>>>()
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            (entry.1.clone(), rows)
        })
        .collect();
    let identity = connection
        .query_row("PRAGMA application_id", [], |row| row.get(0))
        .unwrap();
    (version(connection).unwrap(), identity, schema, rows)
}

#[test]
fn every_supported_version_and_fresh_creation_converge_through_real_open() {
    let directory = tempfile::tempdir().unwrap();
    let fresh = PlanDatabase::open(directory.path().join("fresh.db")).unwrap();
    let expected = admission::schema(&fresh.connection.lock()).unwrap();
    for sql in [
        include_str!("../../../tests/fixtures/releases/supported-v0.sql"),
        include_str!("../../../tests/fixtures/releases/v1.18.6.sql"),
        include_str!("../../../tests/fixtures/releases/v1.18.7.sql"),
        include_str!("../../../tests/fixtures/releases/forward-v2.sql"),
        include_str!("../../../tests/fixtures/releases/forward-v3.sql"),
        include_str!("../../../tests/fixtures/releases/forward-v4.sql"),
        include_str!("../../../tests/fixtures/releases/forward-v5.sql"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("plans.db");
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch(sql).unwrap();
        drop(connection);
        let database = PlanDatabase::open(path).unwrap();
        store_db::release_upgrade_tests::assert_records(&database);
        assert_eq!(
            admission::schema(&database.connection.lock()).unwrap(),
            expected
        );
        assert_eq!(
            version(&database.connection.lock()).unwrap(),
            TARGET_VERSION
        );
    }
}

#[test]
fn a_step_failure_rolls_back_ddl_data_and_version_for_every_source() {
    for fixture in [
        "",
        include_str!("../../../tests/fixtures/releases/supported-v0.sql"),
        include_str!("../../../tests/fixtures/releases/v1.18.6.sql"),
        include_str!("../../../tests/fixtures/releases/v1.18.7.sql"),
        include_str!("../../../tests/fixtures/releases/forward-v2.sql"),
        include_str!("../../../tests/fixtures/releases/forward-v3.sql"),
        include_str!("../../../tests/fixtures/releases/forward-v4.sql"),
    ] {
        let mut connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(fixture).unwrap();
        let before = snapshot(&connection);
        let result = migrate_with(&mut connection, None, |transaction| {
            transaction.execute_batch("INSERT INTO plans(id,title,status,project_path,updated_at,header_json) VALUES ('rolled-back','test','active','/test',0,'{}'); CREATE TABLE rollback_probe(id TEXT);").map_err(db_error)?;
            transaction
                .execute_batch("INSERT INTO deterministic_missing_table VALUES (1)")
                .map_err(db_error)
        });
        assert!(result.is_err());
        assert_eq!(snapshot(&connection), before);
        if !fixture.is_empty() {
            store_db::release_upgrade_tests::assert_input_records(
                &connection,
                version(&connection).unwrap() == 0,
            );
            let count: i64 = connection
                .query_row("SELECT count(*) FROM plans", [], |row| row.get(0))
                .unwrap();
            assert_eq!(count, 1);
        }
    }
}

#[test]
fn unique_constraint_step_conflict_preserves_original_rows_and_version() {
    let mut connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch(include_str!("../../../tests/fixtures/releases/v1.18.7.sql"))
        .unwrap();
    let before = snapshot(&connection);
    let result = migrate_with(&mut connection, None, |transaction| {
        transaction.execute_batch("INSERT INTO plans(id,title,status,project_path,updated_at,header_json) SELECT 'conflict',title,'paused',project_path,updated_at,header_json FROM plans;").map_err(db_error)?;
        apply_step(
            transaction,
            &Step {
                version: TARGET_VERSION + 1,
                sql: "CREATE UNIQUE INDEX unique_plan_titles ON plans(title)",
                schema: "",
                preconditions: &[(
                    "SELECT EXISTS(SELECT title FROM plans GROUP BY title HAVING count(*) > 1)",
                    "duplicate plan titles",
                )],
            },
        )
    });
    assert!(result.is_err());
    assert_eq!(snapshot(&connection), before);
    store_db::release_upgrade_tests::assert_input_records(&connection, false);
    let count: i64 = connection
        .query_row("SELECT count(*) FROM plans", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn invalid_json_during_expression_index_creation_leaves_the_source_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("plans.db");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(include_str!("../../../tests/fixtures/releases/v1.18.6.sql"))
        .unwrap();
    connection
        .execute(
            "UPDATE plan_task_milestones SET row_json = 'invalid-json'",
            [],
        )
        .unwrap();
    let before = snapshot(&connection);
    drop(connection);
    assert!(PlanDatabase::open(path.clone()).is_err());
    let connection = Connection::open(&path).unwrap();
    assert_eq!(snapshot(&connection), before);
}

#[test]
fn foreign_identity_and_undeclared_schema_changes_are_refused_unchanged() {
    for mutation in [
        "PRAGMA application_id = 1234",
        "ALTER TABLE plan_tasks ADD COLUMN unknown TEXT",
        "CREATE TRIGGER unknown AFTER UPDATE ON plans BEGIN SELECT 1; END",
        "DROP TABLE plan_fts; CREATE VIRTUAL TABLE plan_fts USING fts5(plan_id, unexpected)",
        "DROP INDEX plans_owner_tab_id",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("plans.db");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(include_str!("../../../tests/fixtures/releases/v1.18.6.sql"))
            .unwrap();
        connection.execute_batch(mutation).unwrap();
        let before = snapshot(&connection);
        drop(connection);
        assert!(PlanDatabase::open(path.clone()).is_err(), "{mutation}");
        assert_eq!(
            snapshot(&Connection::open(&path).unwrap()),
            before,
            "{mutation}"
        );
    }
}
