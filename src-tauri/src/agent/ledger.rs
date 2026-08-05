//! Durable orchestration ledger — the sqlite truth behind the in-memory
//! [`ReportStore`](super::reports::ReportStore) (wiped on restart). One row per
//! `(orchestrator, worker)`, UPSERTed on every report so a recovered orchestrator
//! can query exactly what it spawned and each worker's last status.

use parking_lot::Mutex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use super::reports::ReportStatus;

const CURRENT_LEDGER_SCHEMA_VERSION: u32 = 3;

/// One worker's latest state under an orchestrator (structured report fields + identity + model).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct LedgerRow {
    pub orchestrator_tab_id: String,
    pub worker_tab_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<ReportStatus>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changed_files: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_cmd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_result: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped: Option<String>,
    /// Unix seconds of the last report. Doubles as last-seen.
    pub updated_at: u64,
}

/// sqlite-backed orchestration ledger. Cheap to clone-share behind an `Arc`.
pub struct OrchestrationLedger {
    conn: Mutex<Connection>,
}

impl OrchestrationLedger {
    /// Open (or create) the ledger db under `data_dir/orchestration.db` (idempotent).
    pub fn open(data_dir: &std::path::Path) -> Result<Self, String> {
        std::fs::create_dir_all(data_dir).map_err(|e| format!("create data dir: {}", e))?;
        let db_path = data_dir.join("orchestration.db");
        let conn =
            Connection::open(&db_path).map_err(|e| format!("open orchestration.db: {}", e))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("set busy_timeout: {}", e))?;
        Self::init_schema(&conn).map_err(|e| format!("init ledger schema: {}", e))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Open a file-backed ledger at an explicit path (durability tests + callers that own the path).
    pub fn open_at(db_path: &std::path::Path) -> Result<Self, String> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {}", e))?;
        }
        let conn = Connection::open(db_path).map_err(|e| format!("open ledger db: {}", e))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("set busy_timeout: {}", e))?;
        Self::init_schema(&conn).map_err(|e| format!("init ledger schema: {}", e))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            "#,
        )?;
        let user_version = Self::user_version(conn)?;
        Self::migrate(conn, user_version)?;
        Ok(())
    }

    fn migrate(conn: &Connection, from_version: u32) -> rusqlite::Result<()> {
        let mut version = from_version;
        loop {
            match version {
                0 => {
                    Self::create_v1_schema(conn)?;
                    version = 1;
                }
                1 => {
                    Self::migrate_v1_to_v2(conn)?;
                    version = 2;
                }
                2 => {
                    Self::migrate_v2_to_v3(conn)?;
                    version = 3;
                }
                CURRENT_LEDGER_SCHEMA_VERSION => return Ok(()),
                _ if version > CURRENT_LEDGER_SCHEMA_VERSION => {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                _ => return Err(rusqlite::Error::InvalidQuery),
            }
        }
    }

    fn create_v1_schema(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS orchestration_ledger (
                orchestrator_tab_id TEXT NOT NULL,
                worker_tab_id       TEXT NOT NULL,
                title                TEXT NOT NULL DEFAULT '',
                branch               TEXT,
                model                TEXT,
                status               TEXT,
                message              TEXT NOT NULL DEFAULT '',
                sha                  TEXT,
                changed_files_json   TEXT,
                test_cmd             TEXT,
                test_result          TEXT,
                skipped              TEXT,
                updated_at           INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (orchestrator_tab_id, worker_tab_id)
            );

            CREATE INDEX IF NOT EXISTS idx_ledger_orchestrator
                ON orchestration_ledger(orchestrator_tab_id);

            PRAGMA user_version = 1;
            "#,
        )
    }

    fn migrate_v1_to_v2(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            r#"
            ALTER TABLE orchestration_ledger ADD COLUMN plan_id TEXT;
            ALTER TABLE orchestration_ledger ADD COLUMN task_id TEXT;
            PRAGMA user_version = 2;
            "#,
        )
    }

    fn migrate_v2_to_v3(conn: &Connection) -> rusqlite::Result<()> {
        if Self::has_column(conn, "orchestrator_pane_id")? {
            conn.execute(
                "ALTER TABLE orchestration_ledger RENAME COLUMN orchestrator_pane_id TO orchestrator_tab_id",
                [],
            )?;
        }
        if Self::has_column(conn, "worker_pane_id")? {
            conn.execute(
                "ALTER TABLE orchestration_ledger RENAME COLUMN worker_pane_id TO worker_tab_id",
                [],
            )?;
        }
        if !Self::has_column(conn, "orchestrator_tab_id")?
            || !Self::has_column(conn, "worker_tab_id")?
        {
            return Err(rusqlite::Error::InvalidQuery);
        }
        conn.execute_batch(
            r#"
            CREATE INDEX IF NOT EXISTS idx_ledger_orchestrator
                ON orchestration_ledger(orchestrator_tab_id);
            PRAGMA user_version = 3;
            "#,
        )
    }

    fn has_column(conn: &Connection, column: &str) -> rusqlite::Result<bool> {
        let mut statement = conn.prepare("PRAGMA table_info(orchestration_ledger)")?;
        let names = statement.query_map([], |row| row.get::<_, String>(1))?;
        for name in names {
            if name? == column {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn user_version(conn: &Connection) -> rusqlite::Result<u32> {
        conn.query_row("PRAGMA user_version", [], |r| {
            let version = r.get::<_, i64>(0)?;
            Ok(version as u32)
        })
    }

    /// UPSERT a worker's latest state — the `(orchestrator, worker)` PK keeps exactly one row per worker, latest wins.
    pub fn upsert(&self, row: &LedgerRow) -> Result<(), String> {
        let changed_files_json = row
            .changed_files
            .as_ref()
            .map(|f| serde_json::to_string(f).unwrap_or_else(|_| "[]".to_string()));
        let status = row.status.map(|s| s.label().to_ascii_lowercase());
        let conn = self.conn.lock();
        conn.execute(
            r#"
            INSERT INTO orchestration_ledger (
                orchestrator_tab_id, worker_tab_id, title, plan_id, task_id, branch, model,
                status, message, sha, changed_files_json, test_cmd,
                test_result, skipped, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
            ON CONFLICT(orchestrator_tab_id, worker_tab_id) DO UPDATE SET
                title = excluded.title,
                plan_id = excluded.plan_id,
                task_id = excluded.task_id,
                branch = excluded.branch,
                model = excluded.model,
                status = excluded.status,
                message = excluded.message,
                sha = excluded.sha,
                changed_files_json = excluded.changed_files_json,
                test_cmd = excluded.test_cmd,
                test_result = excluded.test_result,
                skipped = excluded.skipped,
                updated_at = excluded.updated_at
            "#,
            params![
                row.orchestrator_tab_id,
                row.worker_tab_id,
                row.title,
                row.plan_id,
                row.task_id,
                row.branch,
                row.model,
                status,
                row.message,
                row.sha,
                changed_files_json,
                row.test_cmd,
                row.test_result,
                row.skipped,
                row.updated_at as i64,
            ],
        )
        .map_err(|e| format!("ledger upsert: {}", e))?;
        Ok(())
    }

    /// All workers under an orchestrator, most-recently-seen first.
    pub fn list_for_orchestrator(
        &self,
        orchestrator_tab_id: &str,
    ) -> Result<Vec<LedgerRow>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                r#"
                SELECT orchestrator_tab_id, worker_tab_id, title, plan_id, task_id, branch, model,
                       status, message, sha, changed_files_json, test_cmd,
                       test_result, skipped, updated_at
                FROM orchestration_ledger
                WHERE orchestrator_tab_id = ?1
                ORDER BY updated_at DESC
                "#,
            )
            .map_err(|e| format!("ledger list prepare: {}", e))?;
        let rows = stmt
            .query_map(params![orchestrator_tab_id], Self::row_from_sql)
            .map_err(|e| format!("ledger list query: {}", e))?
            .filter_map(Result::ok)
            .collect();
        Ok(rows)
    }

    /// Completed lanes across every orchestrator, most-recently-finished first.
    /// A durable Done surface: rows that reported `done` or `in_review` stay
    /// inspectable after the pane closes and vanishes from the live sidebar.
    pub fn list_completed(&self, limit: usize) -> Result<Vec<LedgerRow>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                r#"
                SELECT orchestrator_tab_id, worker_tab_id, title, plan_id, task_id, branch, model,
                       status, message, sha, changed_files_json, test_cmd,
                       test_result, skipped, updated_at
                FROM orchestration_ledger
                WHERE status IN ('done', 'in_review')
                ORDER BY updated_at DESC
                LIMIT ?1
                "#,
            )
            .map_err(|e| format!("ledger completed prepare: {}", e))?;
        let rows = stmt
            .query_map(params![limit as i64], Self::row_from_sql)
            .map_err(|e| format!("ledger completed query: {}", e))?
            .filter_map(Result::ok)
            .collect();
        Ok(rows)
    }

    fn row_from_sql(r: &rusqlite::Row<'_>) -> rusqlite::Result<LedgerRow> {
        let status: Option<String> = r.get(7)?;
        let changed_files_json: Option<String> = r.get(10)?;
        Ok(LedgerRow {
            orchestrator_tab_id: r.get(0)?,
            worker_tab_id: r.get(1)?,
            title: r.get(2)?,
            plan_id: r.get(3)?,
            task_id: r.get(4)?,
            branch: r.get(5)?,
            model: r.get(6)?,
            status: status.and_then(|s| parse_status(&s)),
            message: r.get(8)?,
            sha: r.get(9)?,
            changed_files: changed_files_json
                .and_then(|j| serde_json::from_str::<Vec<String>>(&j).ok()),
            test_cmd: r.get(11)?,
            test_result: r.get(12)?,
            skipped: r.get(13)?,
            updated_at: r.get::<_, i64>(14)? as u64,
        })
    }
}

fn parse_status(s: &str) -> Option<ReportStatus> {
    match s.to_ascii_lowercase().as_str() {
        "done" => Some(ReportStatus::Done),
        "in_review" => Some(ReportStatus::InReview),
        "blocked" => Some(ReportStatus::Blocked),
        "partial" => Some(ReportStatus::Partial),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
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

    fn create_pre_versioning_schema(conn: &Connection) {
        conn.execute_batch(
            r#"
            CREATE TABLE orchestration_ledger (
                orchestrator_tab_id TEXT NOT NULL,
                worker_tab_id       TEXT NOT NULL,
                title                TEXT NOT NULL DEFAULT '',
                branch               TEXT,
                model                TEXT,
                status               TEXT,
                message              TEXT NOT NULL DEFAULT '',
                sha                  TEXT,
                changed_files_json   TEXT,
                test_cmd             TEXT,
                test_result          TEXT,
                skipped              TEXT,
                updated_at           INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (orchestrator_tab_id, worker_tab_id)
            );
            "#,
        )
        .unwrap();
    }

    fn create_legacy_v2_schema(conn: &Connection) {
        conn.execute_batch(
            r#"
            CREATE TABLE orchestration_ledger (
                orchestrator_pane_id TEXT NOT NULL,
                worker_pane_id       TEXT NOT NULL,
                title                TEXT NOT NULL DEFAULT '',
                branch               TEXT,
                model                TEXT,
                status               TEXT,
                message              TEXT NOT NULL DEFAULT '',
                sha                  TEXT,
                changed_files_json   TEXT,
                test_cmd             TEXT,
                test_result          TEXT,
                skipped              TEXT,
                updated_at           INTEGER NOT NULL DEFAULT 0,
                plan_id              TEXT,
                task_id              TEXT,
                PRIMARY KEY (orchestrator_pane_id, worker_pane_id)
            );
            CREATE INDEX idx_ledger_orchestrator
                ON orchestration_ledger(orchestrator_pane_id);
            INSERT INTO orchestration_ledger (
                orchestrator_pane_id, worker_pane_id, title, branch, model,
                status, message, sha, changed_files_json, test_cmd,
                test_result, skipped, updated_at, plan_id, task_id
            ) VALUES ('orch', 'w1', 'Worker', 'main', 'opus', 'done', 'msg',
                      'deadbeef', '["a.rs"]', 'cargo test', 'ok', NULL, 42,
                      'plan-a', 'task-a');
            PRAGMA user_version = 2;
            "#,
        )
        .unwrap();
    }

    #[test]
    fn list_completed_returns_only_finished_lanes_newest_first() {
        let db = temp_db();
        let ledger = OrchestrationLedger::open_at(&db).unwrap();
        ledger.upsert(&row("orch_a", "w_done", ReportStatus::Done, 300)).unwrap();
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
        ledger.upsert(&row("orch", "w1", ReportStatus::Done, 100)).unwrap();
        ledger.upsert(&row("orch", "w2", ReportStatus::Done, 200)).unwrap();
        ledger.upsert(&row("orch", "w3", ReportStatus::Done, 300)).unwrap();
        let completed = ledger.list_completed(2).unwrap();
        assert_eq!(completed.len(), 2);
        assert_eq!(completed[0].worker_tab_id, "w3");
        assert_eq!(completed[1].worker_tab_id, "w2");
    }

    fn insert_pre_versioning_row(conn: &Connection) {
        conn.execute(
            r#"
            INSERT INTO orchestration_ledger (
                orchestrator_tab_id, worker_tab_id, title, branch, model,
                status, message, sha, changed_files_json, test_cmd,
                test_result, skipped, updated_at
            ) VALUES ('orch', 'w1', 'Worker', 'main', 'opus', 'done', 'msg',
                      'deadbeef', '["a.rs"]', 'cargo test', 'ok', NULL, 42)
            "#,
            [],
        )
        .unwrap();
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
    fn fresh_db_migrates_to_current_schema_version() {
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
    fn pre_versioning_db_migrates_to_current_without_dropping_rows() {
        let db = temp_db();
        std::fs::create_dir_all(db.parent().unwrap()).unwrap();
        {
            let conn = Connection::open(&db).unwrap();
            create_pre_versioning_schema(&conn);
            insert_pre_versioning_row(&conn);
            assert_eq!(OrchestrationLedger::user_version(&conn).unwrap(), 0);
        }

        let ledger = OrchestrationLedger::open_at(&db).unwrap();
        let rows = ledger.list_for_orchestrator("orch").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].worker_tab_id, "w1");
        assert_eq!(rows[0].plan_id, None);
        assert_eq!(rows[0].task_id, None);
        assert_eq!(rows[0].updated_at, 42);
        let conn = ledger.conn.lock();
        assert_eq!(
            OrchestrationLedger::user_version(&conn).unwrap(),
            CURRENT_LEDGER_SCHEMA_VERSION
        );
        drop(conn);
        std::fs::remove_dir_all(db.parent().unwrap()).ok();
    }

    #[test]
    fn legacy_v2_pane_columns_migrate_without_dropping_rows() {
        let db = temp_db();
        std::fs::create_dir_all(db.parent().unwrap()).unwrap();
        {
            let conn = Connection::open(&db).unwrap();
            create_legacy_v2_schema(&conn);
        }

        let ledger = OrchestrationLedger::open_at(&db).unwrap();
        let rows = ledger.list_for_orchestrator("orch").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].worker_tab_id, "w1");
        assert_eq!(rows[0].plan_id.as_deref(), Some("plan-a"));
        assert_eq!(rows[0].task_id.as_deref(), Some("task-a"));
        ledger
            .upsert(&row("orch", "w1", ReportStatus::Done, 100))
            .unwrap();
        let conn = ledger.conn.lock();
        assert!(OrchestrationLedger::has_column(&conn, "orchestrator_tab_id").unwrap());
        assert!(OrchestrationLedger::has_column(&conn, "worker_tab_id").unwrap());
        assert!(!OrchestrationLedger::has_column(&conn, "orchestrator_pane_id").unwrap());
        assert_eq!(
            OrchestrationLedger::user_version(&conn).unwrap(),
            CURRENT_LEDGER_SCHEMA_VERSION
        );
        drop(conn);
        std::fs::remove_dir_all(db.parent().unwrap()).ok();
    }

    #[test]
    fn migrate_ladder_establishes_current_from_zero() {
        let conn = Connection::open_in_memory().unwrap();
        assert_eq!(OrchestrationLedger::user_version(&conn).unwrap(), 0);

        OrchestrationLedger::migrate(&conn, 0).unwrap();

        assert!(table_exists(&conn, "orchestration_ledger"));
        assert_eq!(
            OrchestrationLedger::user_version(&conn).unwrap(),
            CURRENT_LEDGER_SCHEMA_VERSION
        );
    }
}
