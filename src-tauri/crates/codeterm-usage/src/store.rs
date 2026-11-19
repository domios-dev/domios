use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub use crate::contract::UsageSnapshotRow;
use crate::UsageSample;

use super::suspicious::{
    bounded_raw_excerpt, rejected_debug_json, suspicious_codex_reason, AcceptedUsageSnapshot,
    UsageGuardSnapshot,
};

const SCHEMA: &str = include_str!("schema.sql");
const MAX_ROWS_PER_PROVIDER: i64 = 10_000;

mod reset_times;

// Keep up to a 90-day baseline (~30 samples/day) per provider.
// Storage is small (~80B per row).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageFailureKind {
    Timeout,
    FetchError,
    /// The provider plugin is confined and was denied a sandbox capability it
    /// needs. Distinct from FetchError: nothing upstream is wrong, the plugin
    /// simply cannot reach what it declared, and only the user can fix that.
    CapabilityDenied,
    SourceUnavailable,
    Internal,
    ParseError,
    Skip,
    NoParser,
    DbInsert,
}

impl UsageFailureKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Timeout => "TIMEOUT",
            Self::FetchError => "FETCH_ERROR",
            Self::CapabilityDenied => "CAPABILITY_DENIED",
            Self::SourceUnavailable => "SOURCE_UNAVAILABLE",
            Self::Internal => "INTERNAL",
            Self::ParseError => "PARSE_ERROR",
            Self::Skip => "SKIP",
            Self::NoParser => "NO_PARSER",
            Self::DbInsert => "DB_INSERT",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyRollup {
    pub day: String,
    pub avg_session_pct: Option<f32>,
    pub max_session_pct: Option<f32>,
    pub max_weekly_pct: Option<f32>,
}

pub struct UsageStore {
    conn: Mutex<Connection>,
    db_path: PathBuf,
    cap: i64,
}

pub type UsageStoreHandle = Arc<UsageStore>;

impl UsageStore {
    pub fn open(data_dir: &Path) -> Result<UsageStoreHandle, String> {
        let dir = data_dir.join("usage");
        std::fs::create_dir_all(&dir).map_err(|e| format!("create usage dir: {}", e))?;
        let db_path = dir.join("usage.db");
        let conn = Connection::open(&db_path).map_err(|e| format!("open usage.db: {}", e))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("set busy_timeout: {}", e))?;
        conn.execute_batch(SCHEMA)
            .map_err(|e| format!("init usage schema: {}", e))?;
        add_missing_columns(&conn)?;
        drop_legacy_columns(&conn)?;
        Ok(Arc::new(Self {
            conn: Mutex::new(conn),
            db_path,
            cap: MAX_ROWS_PER_PROVIDER,
        }))
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<UsageStoreHandle, String> {
        Self::open_in_memory_with_cap(MAX_ROWS_PER_PROVIDER)
    }

    #[cfg(test)]
    pub fn open_in_memory_with_cap(cap: i64) -> Result<UsageStoreHandle, String> {
        let conn = Connection::open_in_memory().map_err(|e| format!("open :memory:: {}", e))?;
        conn.execute_batch(SCHEMA)
            .map_err(|e| format!("init usage schema: {}", e))?;
        Ok(Arc::new(Self {
            conn: Mutex::new(conn),
            db_path: PathBuf::from(":memory:"),
            cap,
        }))
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn with_conn<R>(&self, f: impl FnOnce(&Connection) -> R) -> R {
        let conn = self.conn.lock();
        f(&*conn)
    }

    pub fn insert_snapshot(&self, snapshot: &UsageSample) -> Result<i64, String> {
        self.insert_sample(snapshot, false)
    }

    pub fn insert_runtime_snapshot(&self, snapshot: &UsageSample) -> Result<i64, String> {
        self.insert_sample(snapshot, true)
    }

    fn insert_sample(
        &self,
        snapshot: &UsageSample,
        validated_runtime: bool,
    ) -> Result<i64, String> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction().map_err(|e| format!("begin tx: {}", e))?;
        let rejection = if validated_runtime {
            None
        } else {
            suspicious_snapshot_reason(&tx, snapshot)?
        };
        let id = if let Some(reason) = rejection {
            insert_rejected_snapshot(&tx, snapshot, &reason, None)?
        } else {
            insert_header(&tx, snapshot)?
        };
        trim_to_cap(&tx, &snapshot.provider, self.cap)?;
        tx.commit().map_err(|e| format!("commit: {}", e))?;
        Ok(id)
    }

    pub fn insert_failure(
        &self,
        provider: &str,
        account_id: Option<&str>,
        raw_text: Option<&str>,
        error: &str,
        probe_command: Option<&str>,
    ) -> Result<i64, String> {
        self.insert_failure_kind(
            provider,
            account_id,
            raw_text,
            UsageFailureKind::ParseError,
            error,
            probe_command,
        )
    }

    pub fn insert_failure_kind(
        &self,
        provider: &str,
        account_id: Option<&str>,
        _raw_text: Option<&str>,
        kind: UsageFailureKind,
        detail: &str,
        probe_command: Option<&str>,
    ) -> Result<i64, String> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction().map_err(|e| format!("begin tx: {}", e))?;
        let now_ms = chrono::Utc::now().timestamp_millis();
        let reason = format!("{}: {}", kind.as_str(), detail);
        tx.execute(
            r#"INSERT INTO usage_snapshots
               (provider, account_id, captured_at, parsed_ok, failure_reason, probe_command,
                raw_excerpt, debug_json)
               VALUES (?1, ?2, ?3, 0, ?4, ?5, ?6, ?7)"#,
            params![
                provider,
                account_id,
                now_ms,
                reason,
                probe_command,
                _raw_text.map(bounded_raw_excerpt),
                failure_debug_json(&reason, _raw_text),
            ],
        )
        .map_err(|e| format!("insert failure: {}", e))?;
        let id = tx.last_insert_rowid();
        trim_to_cap(&tx, provider, self.cap)?;
        tx.commit().map_err(|e| format!("commit: {}", e))?;
        Ok(id)
    }

    pub fn get_latest(
        &self,
        provider: &str,
        account_id: Option<&str>,
    ) -> Result<Option<UsageSnapshotRow>, String> {
        let conn = self.conn.lock();
        let sql = format!(
            "{} WHERE provider = ?1 AND account_id IS ?2 AND parsed_ok = 1 ORDER BY captured_at DESC LIMIT 1",
            SELECT_HEADER
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| format!("prepare get_latest: {}", e))?;
        let row = stmt
            .query_row(params![provider, account_id], row_to_header)
            .ok();
        Ok(row)
    }

    fn latest_at(
        &self,
        provider: &str,
        account_id: Option<&str>,
        now: i64,
    ) -> Result<Option<UsageSnapshotRow>, String> {
        let mut row = self.get_latest(provider, account_id)?;
        if let Some(row) = &mut row {
            reset_times::apply(&self.conn.lock(), row, now)?;
        }
        Ok(row)
    }

    /// Newest failed probe, whether or not a good reading came after it.
    pub fn get_latest_failure(
        &self,
        provider: &str,
        account_id: Option<&str>,
    ) -> Result<Option<UsageSnapshotRow>, String> {
        let conn = self.conn.lock();
        let sql = format!(
            "{} WHERE provider = ?1 AND account_id IS ?2 AND parsed_ok = 0 ORDER BY captured_at DESC LIMIT 1",
            SELECT_HEADER
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| format!("prepare get_latest_failure: {}", e))?;
        Ok(stmt
            .query_row(params![provider, account_id], row_to_header)
            .ok())
    }

    /// The latest good reading, carrying the reason it stopped moving.
    ///
    /// A frozen gauge with nothing to explain it is the failure this exists to
    /// prevent: when the newest probe failed AFTER the newest good reading, the
    /// row the UI already renders comes back stamped with that failure, so the
    /// staleness has a stated cause instead of being silent.
    pub fn get_latest_with_stale_reason(
        &self,
        provider: &str,
        account_id: Option<&str>,
    ) -> Result<Option<UsageSnapshotRow>, String> {
        let Some(mut latest) =
            self.latest_at(provider, account_id, chrono::Utc::now().timestamp_millis())?
        else {
            return Ok(None);
        };
        if let Some(failure) = self.get_latest_failure(provider, account_id)? {
            if failure.captured_at > latest.captured_at {
                latest.failure_reason = failure.failure_reason;
            }
        }
        Ok(Some(latest))
    }

    /// The row a reader that must never wait silently draws: the latest good
    /// reading with its stale reason, or the newest failure when none ever succeeded.
    pub fn get_latest_or_failure(
        &self,
        provider: &str,
        account_id: Option<&str>,
    ) -> Result<Option<UsageSnapshotRow>, String> {
        match self.get_latest_with_stale_reason(provider, account_id)? {
            Some(latest) => Ok(Some(latest)),
            None => self.get_latest_failure(provider, account_id),
        }
    }

    pub fn get_history(
        &self,
        provider: &str,
        since_ms: i64,
        limit: i64,
        account_id: Option<&str>,
    ) -> Result<Vec<UsageSnapshotRow>, String> {
        self.history(provider, since_ms, limit, account_id, false)
    }
    pub fn get_provider_history(
        &self,
        provider: &str,
        since_ms: i64,
        limit: i64,
    ) -> Result<Vec<UsageSnapshotRow>, String> {
        self.history(provider, since_ms, limit, None, true)
    }

    fn history(
        &self,
        provider: &str,
        since_ms: i64,
        limit: i64,
        account_id: Option<&str>,
        all_accounts: bool,
    ) -> Result<Vec<UsageSnapshotRow>, String> {
        let conn = self.conn.lock();
        let sql = format!(
            "{} WHERE provider = ?1 AND captured_at >= ?2 AND (?5 OR account_id IS ?3) ORDER BY captured_at DESC LIMIT ?4",
            SELECT_HEADER
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| format!("prepare get_history: {}", e))?;
        let rows: Vec<UsageSnapshotRow> = stmt
            .query_map(
                params![provider, since_ms, account_id, limit, all_accounts],
                row_to_header,
            )
            .map_err(|e| format!("query history: {}", e))?
            .filter_map(Result::ok)
            .collect();
        Ok(rows)
    }

    pub fn get_daily_rollup(
        &self,
        provider: &str,
        since_ms: i64,
        account_id: Option<&str>,
    ) -> Result<Vec<DailyRollup>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                r#"SELECT date(captured_at/1000,'unixepoch','localtime') AS day,
                          AVG(session_pct), MAX(session_pct), MAX(weekly_pct)
                   FROM usage_snapshots
                   WHERE provider = ?1 AND captured_at >= ?2 AND parsed_ok = 1 AND account_id IS ?3
                   GROUP BY day
                   ORDER BY day"#,
            )
            .map_err(|e| format!("prepare rollup: {}", e))?;
        let out: Vec<DailyRollup> = stmt
            .query_map(params![provider, since_ms, account_id], |r| {
                Ok(DailyRollup {
                    day: r.get(0)?,
                    avg_session_pct: r.get::<_, Option<f64>>(1)?.map(|v| v as f32),
                    max_session_pct: r.get::<_, Option<f64>>(2)?.map(|v| v as f32),
                    max_weekly_pct: r.get::<_, Option<f64>>(3)?.map(|v| v as f32),
                })
            })
            .map_err(|e| format!("query rollup: {}", e))?
            .filter_map(Result::ok)
            .collect();
        Ok(out)
    }

    pub fn prune_older_than(&self, cutoff_ms: i64) -> Result<usize, String> {
        let conn = self.conn.lock();
        let n = conn
            .execute(
                "DELETE FROM usage_snapshots WHERE captured_at < ?1",
                params![cutoff_ms],
            )
            .map_err(|e| format!("prune: {}", e))?;
        Ok(n)
    }
}

const SELECT_HEADER: &str = r#"SELECT id, provider, account_id, captured_at, parsed_ok,
        session_pct, session_resets_at,
        weekly_pct, weekly_resets_at,
        spend_cents, spend_limit_cents, spend_remaining_cents, spend_resets_at,
        failure_reason, probe_command, raw_excerpt, debug_json, billing_mode, limits_reported, usage_details
        FROM usage_snapshots"#;

fn row_to_header(r: &rusqlite::Row<'_>) -> rusqlite::Result<UsageSnapshotRow> {
    Ok(UsageSnapshotRow {
        usage_details: r
            .get::<_, Option<String>>(19)?
            .and_then(|raw| serde_json::from_str(&raw).ok()),
        id: r.get(0)?,
        provider: r.get(1)?,
        account_id: r.get(2)?,
        captured_at: r.get(3)?,
        parsed_ok: r.get::<_, i64>(4)? != 0,
        session_pct: r.get::<_, Option<f64>>(5)?.map(|v| v as f32),
        session_resets_at: r.get(6)?,
        weekly_pct: r.get::<_, Option<f64>>(7)?.map(|v| v as f32),
        weekly_resets_at: r.get(8)?,
        spend_cents: r.get(9)?,
        spend_limit_cents: r.get(10)?,
        spend_remaining_cents: r.get(11)?,
        spend_resets_at: r.get(12)?,
        failure_reason: r.get(13)?,
        probe_command: r.get(14)?,
        raw_excerpt: r.get(15)?,
        debug_json: r.get(16)?,
        billing_mode: match r.get::<_, String>(17)?.as_str() {
            "subscription" => crate::contract::BillingMode::Subscription,
            "api" => crate::contract::BillingMode::Api,
            _ => crate::contract::BillingMode::Unknown,
        },
        limits_reported: r.get(18)?,
    })
}

fn trim_to_cap(tx: &rusqlite::Transaction<'_>, provider: &str, cap: i64) -> Result<(), String> {
    tx.execute(
        r#"DELETE FROM usage_snapshots
           WHERE provider = ?1
             AND id NOT IN (
               SELECT id FROM usage_snapshots
               WHERE provider = ?1
               ORDER BY captured_at DESC
               LIMIT ?2
             )"#,
        params![provider, cap],
    )
    .map_err(|e| format!("trim: {}", e))?;
    Ok(())
}

fn insert_header(tx: &rusqlite::Transaction<'_>, snap: &UsageSample) -> Result<i64, String> {
    tx.execute(
        r#"INSERT INTO usage_snapshots
           (provider, account_id, captured_at, parsed_ok,
            session_pct, session_resets_at, weekly_pct, weekly_resets_at,
            spend_cents, spend_limit_cents, spend_remaining_cents, spend_resets_at, billing_mode, limits_reported, usage_details)
           VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)"#,
        params![
            &snap.provider,
            &snap.account_id,
            snap.captured_at_ms,
            snap.session_pct.map(|v| v as f64),
            snap.session_resets_at_ms,
            snap.weekly_pct.map(|v| v as f64),
            snap.weekly_resets_at_ms,
            snap.spend_cents,
            snap.spend_limit_cents,
            snap.spend_remaining_cents,
            snap.spend_resets_at_ms,
            snap.billing_mode.as_str(),
            snap.limits_reported,
            snap.usage_details.as_ref().map(serde_json::Value::to_string),
        ],
    )
    .map_err(|e| format!("insert snapshot: {}", e))?;
    Ok(tx.last_insert_rowid())
}

fn insert_rejected_snapshot(
    tx: &rusqlite::Transaction<'_>,
    snap: &UsageSample,
    reason: &str,
    probe_command: Option<&str>,
) -> Result<i64, String> {
    tx.execute(
        r#"INSERT INTO usage_snapshots
           (provider, account_id, captured_at, parsed_ok,
            session_pct, session_resets_at, weekly_pct, weekly_resets_at,
            spend_cents, spend_limit_cents, spend_remaining_cents, spend_resets_at,
            failure_reason, probe_command, raw_excerpt, debug_json, billing_mode, limits_reported, usage_details)
           VALUES (?1, ?2, ?3, 0, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)"#,
        params![
            &snap.provider,
            &snap.account_id,
            snap.captured_at_ms,
            snap.session_pct.map(|v| v as f64),
            snap.session_resets_at_ms,
            snap.weekly_pct.map(|v| v as f64),
            snap.weekly_resets_at_ms,
            snap.spend_cents,
            snap.spend_limit_cents,
            snap.spend_remaining_cents,
            snap.spend_resets_at_ms,
            reason,
            probe_command,
            bounded_raw_excerpt(&snap.raw_text),
            suspicious_debug_json(reason, snap),
            snap.billing_mode.as_str(),
            snap.limits_reported,
            snap.usage_details.as_ref().map(serde_json::Value::to_string),
        ],
    )
    .map_err(|e| format!("insert rejected snapshot: {}", e))?;
    Ok(tx.last_insert_rowid())
}

fn suspicious_snapshot_reason(
    tx: &rusqlite::Transaction<'_>,
    snap: &UsageSample,
) -> Result<Option<String>, String> {
    if snap.provider != "codex" {
        return Ok(None);
    }
    let current = UsageGuardSnapshot {
        captured_at_ms: snap.captured_at_ms,
        session_pct: snap.session_pct,
        session_resets_at_ms: snap.session_resets_at_ms,
        weekly_pct: snap.weekly_pct,
        weekly_resets_at_ms: snap.weekly_resets_at_ms,
    };
    let previous = previous_accepted_snapshot(tx, snap)?.map(|prev| AcceptedUsageSnapshot {
        captured_at_ms: prev.captured_at,
        session_resets_at_ms: prev.session_resets_at,
        weekly_resets_at_ms: prev.weekly_resets_at,
    });
    Ok(suspicious_codex_reason(current, previous).map(str::to_string))
}

fn previous_accepted_snapshot(
    tx: &rusqlite::Transaction<'_>,
    snap: &UsageSample,
) -> Result<Option<UsageSnapshotRow>, String> {
    let sql = format!(
        "{} WHERE provider = ?1 AND account_id IS ?2 AND parsed_ok = 1 AND captured_at < ?3 ORDER BY captured_at DESC LIMIT 1",
        SELECT_HEADER
    );
    let mut stmt = tx
        .prepare(&sql)
        .map_err(|e| format!("prepare previous accepted: {}", e))?;
    Ok(stmt
        .query_row(
            params![&snap.provider, &snap.account_id, snap.captured_at_ms],
            row_to_header,
        )
        .ok())
}

fn suspicious_debug_json(reason: &str, snap: &UsageSample) -> String {
    rejected_debug_json(
        reason,
        UsageGuardSnapshot {
            captured_at_ms: snap.captured_at_ms,
            session_pct: snap.session_pct,
            session_resets_at_ms: snap.session_resets_at_ms,
            weekly_pct: snap.weekly_pct,
            weekly_resets_at_ms: snap.weekly_resets_at_ms,
        },
        &snap.raw_text,
    )
}

fn failure_debug_json(error: &str, raw_text: Option<&str>) -> String {
    json!({
        "reason": error,
        "raw_excerpt_chars": raw_text.map(|raw| bounded_raw_excerpt(raw).chars().count()),
    })
    .to_string()
}

const LEGACY_COLUMNS: &[&str] = &[
    "weekly_sonnet_pct",
    "weekly_sonnet_resets_at",
    "session_cost_usd",
    "session_api_duration_s",
    "session_wall_duration_s",
    "session_lines_added",
    "session_lines_removed",
    "session_tokens_input",
    "session_tokens_output",
    "session_tokens_cache_read",
    "session_tokens_cache_write",
    "parse_error",
    "raw_text",
];

// (CREATE TABLE IF NOT EXISTS leaves an already-created table untouched).
const ADDED_COLUMNS: &[(&str, &str)] = &[
    ("usage_details", "TEXT"),
    ("billing_mode", "TEXT NOT NULL DEFAULT 'unknown'"),
    ("limits_reported", "INTEGER NOT NULL DEFAULT 0"),
    ("spend_cents", "REAL"),
    ("spend_limit_cents", "REAL"),
    ("spend_remaining_cents", "REAL"),
    ("spend_resets_at", "INTEGER"),
    ("failure_reason", "TEXT"),
    ("probe_command", "TEXT"),
    ("raw_excerpt", "TEXT"),
    ("debug_json", "TEXT"),
];

fn add_missing_columns(conn: &Connection) -> Result<(), String> {
    let existing = table_columns(conn)?;
    for (col, ty) in ADDED_COLUMNS {
        if !existing.contains(*col) {
            conn.execute(
                &format!("ALTER TABLE usage_snapshots ADD COLUMN {} {}", col, ty),
                [],
            )
            .map_err(|e| format!("add {}: {}", col, e))?;
        }
    }
    Ok(())
}

fn table_columns(conn: &Connection) -> Result<std::collections::HashSet<String>, String> {
    let mut stmt = conn
        .prepare("PRAGMA table_info(usage_snapshots)")
        .map_err(|e| format!("pragma: {}", e))?;
    let cols = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(|e| format!("pragma rows: {}", e))?;
    Ok(cols.flatten().collect())
}

fn drop_legacy_columns(conn: &Connection) -> Result<(), String> {
    let existing = table_columns(conn)?;
    for col in LEGACY_COLUMNS {
        if existing.contains(*col) {
            let sql = format!("ALTER TABLE usage_snapshots DROP COLUMN {}", col);
            conn.execute(&sql, [])
                .map_err(|e| format!("drop {}: {}", col, e))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
