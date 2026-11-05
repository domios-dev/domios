use super::*;
use serde_json::Value;
use sha2::{Digest, Sha256};

const MANIFEST: &str = include_str!("../../../tests/fixtures/releases/manifest.json");
const RECORDS: &str = include_str!("../../../tests/fixtures/releases/records.json");
const FIXTURES: &[(&str, &str)] = &[
    (
        "v1.18.6.sql",
        include_str!("../../../tests/fixtures/releases/v1.18.6.sql"),
    ),
    (
        "v1.18.7.sql",
        include_str!("../../../tests/fixtures/releases/v1.18.7.sql"),
    ),
    (
        "forward-v2.sql",
        include_str!("../../../tests/fixtures/releases/forward-v2.sql"),
    ),
    (
        "forward-v3.sql",
        include_str!("../../../tests/fixtures/releases/forward-v3.sql"),
    ),
    (
        "forward-v4.sql",
        include_str!("../../../tests/fixtures/releases/forward-v4.sql"),
    ),
    (
        "forward-v5.sql",
        include_str!("../../../tests/fixtures/releases/forward-v5.sql"),
    ),
    (
        "supported-v0.sql",
        include_str!("../../../tests/fixtures/releases/supported-v0.sql"),
    ),
];

fn digest(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn fingerprint(connection: &Connection) -> String {
    digest(serde_json::to_vec(&admission::schema(connection).unwrap()).unwrap())
}

fn operations(connection: &Connection) -> Value {
    let mut statement = connection
        .prepare("SELECT id, state, finished_at, error FROM plan_operations ORDER BY id")
        .unwrap();
    let rows = statement
        .query_map([], |row| {
            Ok(serde_json::json!([
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<String>>(3)?
            ]))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    Value::Array(rows)
}

pub(in crate::service_plan) fn assert_input_records(connection: &Connection, legacy: bool) {
    let expected: Value = serde_json::from_str(RECORDS).unwrap();
    assert_eq!(operations(connection), expected["operations_before"]);
    let column = if legacy {
        "document_json"
    } else {
        "header_json"
    };
    let document: String = connection
        .query_row(
            &format!("SELECT {column} FROM plans WHERE id = 'pl_release'"),
            [],
            |row| row.get(0),
        )
        .unwrap();
    let mut header = expected["plans"][0].clone();
    for key in if legacy {
        &["tasks"][..]
    } else {
        &["tasks", "sections", "elements", "strides", "heartbeat"][..]
    } {
        header.as_object_mut().unwrap().remove(*key);
    }
    assert_eq!(serde_json::from_str::<Value>(&document).unwrap(), header);
    let column = if legacy { "document_json" } else { "row_json" };
    let document: String = connection
        .query_row(
            &format!("SELECT {column} FROM plan_tasks WHERE task_id = 'task_release'"),
            [],
            |row| row.get(0),
        )
        .unwrap();
    let mut task = expected["plans"][0]["tasks"][0].clone();
    task.as_object_mut().unwrap().remove("milestones");
    task.as_object_mut().unwrap().remove("execution_attempts");
    assert_eq!(serde_json::from_str::<Value>(&document).unwrap(), task);
    let document: String = connection
        .query_row(
            &format!("SELECT {column} FROM specs WHERE id = 'spec_release'"),
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&document).unwrap(),
        expected["specs"][0]
    );
}

pub(in crate::service_plan) fn assert_records(database: &PlanDatabase) {
    let records: Value = serde_json::from_str(RECORDS).unwrap();
    let plans: Vec<Plan> = serde_json::from_value(records["plans"].clone()).unwrap();
    let specs: Vec<Spec> = serde_json::from_value(records["specs"].clone()).unwrap();
    assert_eq!(database.load_plans_for_test(), plans);
    assert_eq!(database.load_specs().unwrap(), specs);
    assert_eq!(
        operations(&database.connection.lock()),
        records["operations_after"]
    );
}

#[test]
fn release_fixtures_upgrade_and_reopen_with_exact_records() {
    let manifest: Value = serde_json::from_str(MANIFEST).unwrap();
    assert_eq!(digest(RECORDS), manifest["records_sha256"]);
    assert_eq!(
        FIXTURES.len(),
        manifest["fixtures"].as_array().unwrap().len()
    );
    let target = Connection::open_in_memory().unwrap();
    create_schema(&target).unwrap();
    for (name, sql) in FIXTURES {
        let entry = manifest["fixtures"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["file"] == *name)
            .unwrap();
        assert_eq!(digest(sql), entry["sha256"], "{name}");
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("plans.db");
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch(sql).unwrap();
        assert_eq!(
            fingerprint(&connection),
            entry["schema_fingerprint"],
            "{name}"
        );
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, entry["user_version"].as_i64().unwrap());
        assert_input_records(&connection, version == 0);
        drop(connection);
        for _ in 0..2 {
            let database = PlanDatabase::open(path.clone()).unwrap();
            assert_records(&database);
            let connection = database.connection.lock();
            assert_eq!(
                admission::schema(&connection).unwrap(),
                admission::schema(&target).unwrap(),
                "{name}"
            );
            assert_eq!(
                connection
                    .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                    .unwrap(),
                SCHEMA_VERSION
            );
            assert_eq!(
                connection
                    .query_row(
                        "SELECT count(*) FROM sqlite_schema WHERE name GLOB 'plan_fts*'",
                        [],
                        |row| row.get::<_, i64>(0)
                    )
                    .unwrap(),
                0
            );
        }
    }
}

#[test]
fn persisted_schema_matches_registered_release_contract() {
    let contract: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/releases/contract.json"
    ))
    .unwrap();
    let connection = Connection::open_in_memory().unwrap();
    create_schema(&connection).unwrap();
    assert_eq!(
        fingerprint(&connection),
        contract["target"]["schema_fingerprint"],
        "Persisted plan schema changed: register a migration and a new immutable release fixture"
    );
    assert_eq!(
        SCHEMA_VERSION,
        contract["target"]["user_version"].as_i64().unwrap()
    );
}
