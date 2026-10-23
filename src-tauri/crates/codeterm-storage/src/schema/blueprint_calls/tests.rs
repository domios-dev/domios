use rusqlite::Connection;
#[test]
fn blueprint_v2_call_additive_journal_migration_retains_old_rows_with_empty_frames() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE blueprint_step_journal (entry_id INTEGER PRIMARY KEY,step_name TEXT);INSERT INTO blueprint_step_journal VALUES(7,'old-node');").unwrap();
    conn.execute_batch(super::SCHEMA).unwrap();
    let (id, name, stack): (i64, String, Option<String>) = conn
        .query_row(
            "SELECT entry_id,step_name,call_stack_json FROM blueprint_step_journal",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((id, name, stack), (7, "old-node".into(), None));
    assert!(conn
        .execute(
            "UPDATE blueprint_step_journal SET call_stack_json=?1",
            ["[{},{},{},{},{},{},{},{},{}]"]
        )
        .is_err());
}

#[test]
fn blueprint_v2_call_journal_run_identity_migration_keeps_old_rows_unknown() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE blueprint_step_journal (entry_id INTEGER PRIMARY KEY);INSERT INTO blueprint_step_journal VALUES(7);").unwrap();
    conn.execute_batch(super::RUN_IDENTITY_SCHEMA).unwrap();
    let row: (i64, Option<i64>) = conn
        .query_row(
            "SELECT entry_id,definition_version FROM blueprint_step_journal",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(row, (7, None));
    assert!(conn
        .execute(
            "UPDATE blueprint_step_journal SET definition_version=-1",
            []
        )
        .is_err());
}
