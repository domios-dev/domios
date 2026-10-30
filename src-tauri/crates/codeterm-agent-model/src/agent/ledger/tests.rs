use super::*;

fn row(orch: &str, worker: &str, status: ReportStatus, at: u64) -> LedgerRow {
    LedgerRow {
        orchestrator_tab_id: orch.into(),
        worker_tab_id: worker.into(),
        title: "Worker".into(),
        plan_id: Some("pl_1".into()),
        task_id: Some("task_1".into()),
        branch: Some("orch-p3-ledger".into()),
        model: Some("opus".into()),
        status: Some(status),
        message: "msg".into(),
        sha: Some("deadbeef".into()),
        changed_files: Some(vec!["a.rs".into(), "b.rs".into()]),
        test_cmd: Some("cargo test".into()),
        test_result: Some("ok".into()),
        skipped: None,
        updated_at: at,
    }
}

fn temp_db() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "ledger-test-{}/orchestration.db",
        ulid::Ulid::new()
    ))
}

#[test]
fn list_completed_returns_only_finished_lanes_newest_first() {
    let db = temp_db();
    let ledger = OrchestrationLedger::open_at(&db).unwrap();
    ledger
        .upsert(&row("orch_a", "w_done", ReportStatus::Done, 300))
        .unwrap();
    ledger
        .upsert(&row("orch_b", "w_review", ReportStatus::InReview, 200))
        .unwrap();
    ledger
        .upsert(&row("orch_a", "w_blocked", ReportStatus::Blocked, 400))
        .unwrap();
    ledger
        .upsert(&row("orch_b", "w_partial", ReportStatus::Partial, 500))
        .unwrap();

    let completed = ledger.list_completed(50).unwrap();
    let workers: Vec<&str> = completed.iter().map(|r| r.worker_tab_id.as_str()).collect();
    assert_eq!(
        workers,
        vec!["w_done", "w_review"],
        "only done/in_review lanes appear, most-recently-finished first, spanning all orchestrators"
    );
}

#[test]
fn list_completed_respects_limit() {
    let db = temp_db();
    let ledger = OrchestrationLedger::open_at(&db).unwrap();
    ledger
        .upsert(&row("orch", "w1", ReportStatus::Done, 100))
        .unwrap();
    ledger
        .upsert(&row("orch", "w2", ReportStatus::Done, 200))
        .unwrap();
    ledger
        .upsert(&row("orch", "w3", ReportStatus::Done, 300))
        .unwrap();
    let completed = ledger.list_completed(2).unwrap();
    assert_eq!(completed.len(), 2);
    assert_eq!(completed[0].worker_tab_id, "w3");
    assert_eq!(completed[1].worker_tab_id, "w2");
}

fn table_exists(conn: &Connection, table_name: &str) -> bool {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        params![table_name],
        |r| r.get::<_, bool>(0),
    )
    .unwrap()
}

#[test]
fn upsert_keeps_one_row_with_latest_status() {
    let db = temp_db();
    let ledger = OrchestrationLedger::open_at(&db).unwrap();

    ledger
        .upsert(&row("orch", "w1", ReportStatus::Partial, 100))
        .unwrap();
    ledger
        .upsert(&row("orch", "w1", ReportStatus::Done, 200))
        .unwrap();

    let listed = ledger.list_for_orchestrator("orch").unwrap();
    assert_eq!(listed.len(), 1, "re-report UPSERTs, not appends");
    assert_eq!(
        listed[0].status,
        Some(ReportStatus::Done),
        "latest status wins"
    );
    assert_eq!(listed[0].updated_at, 200);
    std::fs::remove_dir_all(db.parent().unwrap()).ok();
}

#[test]
fn rows_survive_drop_and_reopen() {
    let db = temp_db();
    {
        let ledger = OrchestrationLedger::open_at(&db).unwrap();
        ledger
            .upsert(&row("orch", "w1", ReportStatus::Done, 1))
            .unwrap();
        ledger
            .upsert(&row("orch", "w2", ReportStatus::Blocked, 2))
            .unwrap();
    } // Connection dropped — simulates daemon restart.

    let reopened = OrchestrationLedger::open_at(&db).unwrap();
    let listed = reopened.list_for_orchestrator("orch").unwrap();
    assert_eq!(listed.len(), 2, "ledger rows persist across reopen");
    // ORDER BY updated_at DESC: w2 first.
    assert_eq!(listed[0].worker_tab_id, "w2");
    assert_eq!(listed[0].status, Some(ReportStatus::Blocked));
    assert_eq!(listed[1].worker_tab_id, "w1");
    assert_eq!(listed[1].status, Some(ReportStatus::Done));
    let conn = Connection::open(&db).unwrap();
    assert_eq!(
        OrchestrationLedger::user_version(&conn).unwrap(),
        CURRENT_LEDGER_SCHEMA_VERSION
    );
    std::fs::remove_dir_all(db.parent().unwrap()).ok();
}

#[test]
fn list_scopes_to_one_orchestrator() {
    let db = temp_db();
    let ledger = OrchestrationLedger::open_at(&db).unwrap();
    ledger
        .upsert(&row("orch-1", "w1", ReportStatus::Done, 1))
        .unwrap();
    ledger
        .upsert(&row("orch-2", "w2", ReportStatus::Done, 1))
        .unwrap();
    assert_eq!(ledger.list_for_orchestrator("orch-1").unwrap().len(), 1);
    assert_eq!(ledger.list_for_orchestrator("orch-2").unwrap().len(), 1);
    assert_eq!(ledger.list_for_orchestrator("nobody").unwrap().len(), 0);
    std::fs::remove_dir_all(db.parent().unwrap()).ok();
}

#[test]
fn structured_fields_round_trip_through_sql() {
    let db = temp_db();
    let ledger = OrchestrationLedger::open_at(&db).unwrap();
    let r = row("orch", "w1", ReportStatus::Done, 42);
    ledger.upsert(&r).unwrap();
    let back = ledger.list_for_orchestrator("orch").unwrap();
    assert_eq!(back[0], r, "every column round-trips through sqlite");
    std::fs::remove_dir_all(db.parent().unwrap()).ok();
}

#[test]
fn reopening_existing_db_is_idempotent() {
    let db = temp_db();
    let l1 = OrchestrationLedger::open_at(&db).unwrap();
    l1.upsert(&row("orch", "w1", ReportStatus::Done, 1))
        .unwrap();
    drop(l1);
    // Second open must not error or wipe existing rows.
    let l2 = OrchestrationLedger::open_at(&db).unwrap();
    assert_eq!(l2.list_for_orchestrator("orch").unwrap().len(), 1);
    let conn = l2.conn.lock();
    assert_eq!(
        OrchestrationLedger::user_version(&conn).unwrap(),
        CURRENT_LEDGER_SCHEMA_VERSION
    );
    drop(conn);
    std::fs::remove_dir_all(db.parent().unwrap()).ok();
}

#[test]
fn fresh_db_creates_current_schema_version() {
    let db = temp_db();
    let ledger = OrchestrationLedger::open_at(&db).unwrap();
    let conn = ledger.conn.lock();
    assert!(table_exists(&conn, "orchestration_ledger"));
    assert_eq!(
        OrchestrationLedger::user_version(&conn).unwrap(),
        CURRENT_LEDGER_SCHEMA_VERSION
    );
    drop(conn);
    std::fs::remove_dir_all(db.parent().unwrap()).ok();
}

#[test]
fn unsupported_schema_is_refused_without_changing_data() {
    for version in [0, 1, 2, 4] {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("orchestration.db");
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE preserved (value TEXT); INSERT INTO preserved VALUES ('history');",
            )
            .unwrap();
            conn.pragma_update(None, "user_version", version).unwrap();
        }
        let before = std::fs::read(&db).unwrap();
        let error = OrchestrationLedger::open_at(&db).err().unwrap();
        assert!(
            error.contains("unsupported orchestration ledger schema version"),
            "{error}"
        );
        assert_eq!(std::fs::read(&db).unwrap(), before);
        let conn = Connection::open(&db).unwrap();
        assert_eq!(OrchestrationLedger::user_version(&conn).unwrap(), version);
        assert_eq!(
            conn.query_row("SELECT value FROM preserved", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "history"
        );
        assert!(!table_exists(&conn, "orchestration_ledger"));
    }
}
