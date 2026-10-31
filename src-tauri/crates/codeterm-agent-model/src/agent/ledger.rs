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

    fn init_schema(conn: &Connection) -> Result<(), String> {
        let version = Self::user_version(conn).map_err(|error| error.to_string())?;
        let populated: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name NOT LIKE 'sqlite_%')",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if version != CURRENT_LEDGER_SCHEMA_VERSION && (version != 0 || populated) {
            return Err(format!(
                "unsupported orchestration ledger schema version {version}; expected {CURRENT_LEDGER_SCHEMA_VERSION}; data was preserved"
            ));
        }
        if version == 0 {
            Self::create_current_schema(conn).map_err(|error| error.to_string())?;
        }
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")
            .map_err(|error| error.to_string())
    }

    fn create_current_schema(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            r#"
            BEGIN IMMEDIATE;
            CREATE TABLE IF NOT EXISTS orchestration_ledger (
                orchestrator_tab_id TEXT NOT NULL,
                worker_tab_id       TEXT NOT NULL,
                title                TEXT NOT NULL DEFAULT '',
                plan_id              TEXT,
                task_id              TEXT,
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

            PRAGMA user_version = 3;
            COMMIT;
            "#,
        )
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
        let status = row
            .status
            .map(|s| s.label().to_ascii_lowercase().replace(' ', "_"));
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
mod tests;
