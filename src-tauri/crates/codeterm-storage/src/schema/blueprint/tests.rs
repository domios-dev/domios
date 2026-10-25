use crate::schema::{blueprint_v2, MIGRATIONS};
use rusqlite::Connection;

#[test]
fn blueprint_rename_preserves_pre_release_rows_and_foreign_keys() {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
    let rename = MIGRATIONS
        .iter()
        .position(|migration| *migration == super::SCHEMA)
        .unwrap();
    let cleanup = MIGRATIONS
        .iter()
        .position(|migration| *migration == blueprint_v2::CLEANUP)
        .unwrap();
    for step in &MIGRATIONS[..rename] {
        conn.execute_batch(step).unwrap();
    }
    conn.execute_batch(r#"
        INSERT INTO sentinels VALUES ('sentinel_old', 0, 'tab', 'workspace', NULL, '/work', 1, 'sentinel_old', 1);
        INSERT INTO sentinel_definitions (sentinel_id, version, provenance, source_kind, probe, question, interval_ms, max_transitions, max_actions, created_revision)
            VALUES ('sentinel_old', 1, 'authored', 'probe', 'status', 'state?', 20000, 64, 256, 1);
        INSERT INTO sentinel_definitions (sentinel_id, version, provenance, source_kind, probe, question, interval_ms, max_transitions, max_actions, created_revision, pipeline_spec)
            VALUES ('sentinel_old', 2, 'authored', 'probe', 'blueprint-pipeline', '', 20000, 64, 256, 1, '{}');
        INSERT INTO sentinel_definitions (sentinel_id, version, provenance, source_kind, probe, observe_kind, observe_ref, question, interval_ms, max_transitions, max_actions, created_revision, terminal_spec)
            VALUES ('sentinel_old', 3, 'authored', 'observe', '', 'tab', 'watched', 'state?', 20000, 64, 256, 1, '{"pattern":"ready","idle_ms":null}');
        INSERT INTO sentinel_heads VALUES ('sentinel_old', 1);
        INSERT INTO sentinel_buckets VALUES ('sentinel_old', 1, 0, 'done', '^done$', 0);
        INSERT INTO sentinel_handlers (sentinel_id, version, bucket, ordinal, handler_id, kind, target_ref)
            VALUES ('sentinel_old', 1, 'done', 0, 'next', 'sentinel', 'sentinel_old');
        INSERT INTO sentinel_handler_targets VALUES ('sentinel_old', 1, 'next', 'sentinel', 'sentinel_old', 'workspace', NULL, 'tab');
        INSERT INTO blueprint_step_journal (sentinel_id, run_id, step_index, step_name, state, started_at_ms, ended_at_ms)
            VALUES ('sentinel_old', 'run', -1, '$run', 'done', 1000, 1001);
        INSERT INTO blueprint_step_journal (sentinel_id, run_id, step_index, step_name, state, started_at_ms, ended_at_ms, result_summary)
            VALUES ('sentinel_old', 'legacy-result', -1, '$result', 'done', 1000, 1001, 'body_ref:inbox/old.md');
    "#).unwrap();
    let tx = conn.transaction().unwrap();
    for step in &MIGRATIONS[rename..cleanup] {
        tx.execute_batch(step).unwrap();
    }
    tx.commit().unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT probe FROM blueprint_definitions WHERE blueprint_id = 'sentinel_old' AND version = 1",
            [],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "status"
    );
    assert_eq!(
        conn.query_row(
            "SELECT source_kind, probe FROM blueprint_definitions WHERE version = 2",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        )
        .unwrap(),
        ("pipeline".into(), String::new())
    );
    assert_eq!(
        conn.query_row(
            "SELECT source_kind FROM blueprint_definitions WHERE version = 3",
            [],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "terminal"
    );
    assert_eq!(
        conn.query_row("SELECT kind FROM blueprint_handlers", [], |row| row
            .get::<_, String>(0))
            .unwrap(),
        "blueprint"
    );
    assert_eq!(
        conn.query_row("SELECT kind FROM blueprint_handler_targets", [], |row| {
            row.get::<_, String>(0)
        })
        .unwrap(),
        "blueprint"
    );
    assert_eq!(
        conn.query_row(
            "SELECT run_id FROM blueprint_step_journal WHERE blueprint_id = 'sentinel_old' AND step_name = '$run'",
            [],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "run"
    );
    assert_eq!(
        conn.query_row(
            "SELECT body_ref FROM blueprint_step_journal WHERE run_id = 'legacy-result'",
            [],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "inbox/old.md"
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table' AND name LIKE 'sentinel%'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
}
