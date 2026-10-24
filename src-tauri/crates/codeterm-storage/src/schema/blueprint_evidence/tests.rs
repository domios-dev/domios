use rusqlite::Connection;

#[test]
fn journal_evidence_migration_retains_old_rows_and_refuses_invalid_or_oversized_objects() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE blueprints(id TEXT PRIMARY KEY); INSERT INTO blueprints VALUES('bp');
        CREATE TABLE blueprint_step_journal(entry_id INTEGER PRIMARY KEY,blueprint_id TEXT,run_id TEXT,step_index INTEGER CHECK(step_index BETWEEN -1 AND 63),step_name TEXT,state TEXT,outcome TEXT,started_at_ms INTEGER,ended_at_ms INTEGER,result_summary TEXT,reason TEXT,body_ref TEXT,call_stack_json TEXT,claim_generation INTEGER,claim_parent_index INTEGER,definition_version INTEGER);
        INSERT INTO blueprint_step_journal VALUES(7,'bp','old-run',0,'node','done','done',1,2,'summary',NULL,NULL,'[]',3,NULL,4);").unwrap();
    conn.execute_batch(super::SCHEMA).unwrap();
    let row: (i64, Option<String>, i64, String) = conn.query_row("SELECT entry_id,evidence_json,definition_version,result_summary FROM blueprint_step_journal", [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).unwrap();
    assert_eq!(row, (7, None, 4, "summary".into()));
    conn.execute(
        "UPDATE blueprint_step_journal SET step_index=70000,evidence_json=?1",
        [r#"{"ports":{"inputs":{},"outputs":{"out":{"preview":"7","type":"number"}}}}"#],
    )
    .unwrap();
    for raw in [
        "invalid".to_string(),
        "[]".into(),
        format!("{{\"padding\":\"{}\"}}", "λ".repeat(16384)),
    ] {
        assert!(conn
            .execute("UPDATE blueprint_step_journal SET evidence_json=?1", [raw])
            .is_err());
    }
    assert!(conn
        .execute("UPDATE blueprint_step_journal SET step_index=-2", [])
        .is_err());
}
