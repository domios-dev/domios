use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::agent::usage_parser::UsageSnapshot;
pub use codeterm_types::usage::UsageSnapshotRow;

use super::suspicious::{
    bounded_raw_excerpt, rejected_debug_json, suspicious_codex_reason, AcceptedUsageSnapshot,
    UsageGuardSnapshot,
};

const SCHEMA: &str = include_str!("schema.sql");
const MAX_ROWS_PER_PROVIDER: i64 = 10_000;

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

    pub fn insert_snapshot(&self, snapshot: &UsageSnapshot) -> Result<i64, String> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction().map_err(|e| format!("begin tx: {}", e))?;
        let id = if let Some(reason) = suspicious_snapshot_reason(&tx, snapshot)? {
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
            "{} WHERE provider = ?1 AND (?2 IS NULL OR account_id = ?2) AND parsed_ok = 1 ORDER BY captured_at DESC LIMIT 1",
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

    /// Newest failed probe, whether or not a good reading came after it.
    pub fn get_latest_failure(
        &self,
        provider: &str,
        account_id: Option<&str>,
    ) -> Result<Option<UsageSnapshotRow>, String> {
        let conn = self.conn.lock();
        let sql = format!(
            "{} WHERE provider = ?1 AND (?2 IS NULL OR account_id = ?2) AND parsed_ok = 0 ORDER BY captured_at DESC LIMIT 1",
            SELECT_HEADER
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| format!("prepare get_latest_failure: {}", e))?;
        Ok(stmt.query_row(params![provider, account_id], row_to_header).ok())
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
        let Some(mut latest) = self.get_latest(provider, account_id)? else {
            return Ok(None);
        };
        if let Some(failure) = self.get_latest_failure(provider, account_id)? {
            if failure.captured_at > latest.captured_at {
                latest.failure_reason = failure.failure_reason;
            }
        }
        Ok(Some(latest))
    }

    pub fn get_history(
        &self,
        provider: &str,
        since_ms: i64,
        limit: i64,
        account_id: Option<&str>,
    ) -> Result<Vec<UsageSnapshotRow>, String> {
        let conn = self.conn.lock();
        let sql = format!(
            "{} WHERE provider = ?1 AND captured_at >= ?2 AND (?3 IS NULL OR account_id = ?3) ORDER BY captured_at DESC LIMIT ?4",
            SELECT_HEADER
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| format!("prepare get_history: {}", e))?;
        let rows: Vec<UsageSnapshotRow> = stmt
            .query_map(
                params![provider, since_ms, account_id, limit],
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
    ) -> Result<Vec<DailyRollup>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                r#"SELECT date(captured_at/1000,'unixepoch','localtime') AS day,
                          AVG(session_pct), MAX(session_pct), MAX(weekly_pct)
                   FROM usage_snapshots
                   WHERE provider = ?1 AND captured_at >= ?2 AND parsed_ok = 1
                   GROUP BY day
                   ORDER BY day"#,
            )
            .map_err(|e| format!("prepare rollup: {}", e))?;
        let out: Vec<DailyRollup> = stmt
            .query_map(params![provider, since_ms], |r| {
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
        failure_reason, probe_command, raw_excerpt, debug_json
        FROM usage_snapshots"#;

fn row_to_header(r: &rusqlite::Row<'_>) -> rusqlite::Result<UsageSnapshotRow> {
    Ok(UsageSnapshotRow {
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

fn insert_header(tx: &rusqlite::Transaction<'_>, snap: &UsageSnapshot) -> Result<i64, String> {
    tx.execute(
        r#"INSERT INTO usage_snapshots
           (provider, account_id, captured_at, parsed_ok,
            session_pct, session_resets_at, weekly_pct, weekly_resets_at,
            spend_cents, spend_limit_cents, spend_remaining_cents, spend_resets_at)
           VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"#,
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
        ],
    )
    .map_err(|e| format!("insert snapshot: {}", e))?;
    Ok(tx.last_insert_rowid())
}

fn insert_rejected_snapshot(
    tx: &rusqlite::Transaction<'_>,
    snap: &UsageSnapshot,
    reason: &str,
    probe_command: Option<&str>,
) -> Result<i64, String> {
    tx.execute(
        r#"INSERT INTO usage_snapshots
           (provider, account_id, captured_at, parsed_ok,
            session_pct, session_resets_at, weekly_pct, weekly_resets_at,
            spend_cents, spend_limit_cents, spend_remaining_cents, spend_resets_at,
            failure_reason, probe_command, raw_excerpt, debug_json)
           VALUES (?1, ?2, ?3, 0, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)"#,
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
        ],
    )
    .map_err(|e| format!("insert rejected snapshot: {}", e))?;
    Ok(tx.last_insert_rowid())
}

fn suspicious_snapshot_reason(
    tx: &rusqlite::Transaction<'_>,
    snap: &UsageSnapshot,
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
    snap: &UsageSnapshot,
) -> Result<Option<UsageSnapshotRow>, String> {
    let sql = format!(
        "{} WHERE provider = ?1 AND (?2 IS NULL OR account_id = ?2) AND parsed_ok = 1 AND captured_at < ?3 ORDER BY captured_at DESC LIMIT 1",
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

fn suspicious_debug_json(reason: &str, snap: &UsageSnapshot) -> String {
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
mod tests {
    use super::*;

    fn make_snapshot(ms: i64) -> UsageSnapshot {
        let mut s = UsageSnapshot::empty("claude", "raw", ms);
        s.session_pct = Some(42.0);
        s.weekly_pct = Some(10.0);
        s
    }

    fn codex_snapshot(
        ms: i64,
        session_pct: f32,
        weekly_pct: f32,
        session_reset: i64,
        weekly_reset: i64,
    ) -> UsageSnapshot {
        let mut s = UsageSnapshot::empty("codex", "OpenAI Codex\nraw usage screen", ms);
        s.session_pct = Some(session_pct);
        s.weekly_pct = Some(weekly_pct);
        s.session_resets_at_ms = Some(session_reset);
        s.weekly_resets_at_ms = Some(weekly_reset);
        s
    }

    #[test]
    fn add_missing_columns_migrates_pre_spend_db() {
        // Pre-spend schema: original columns only, no spend_* yet.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE usage_snapshots (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               provider TEXT NOT NULL, account_id TEXT,
               captured_at INTEGER NOT NULL, parsed_ok INTEGER NOT NULL,
               session_pct REAL, session_resets_at INTEGER,
               weekly_pct REAL, weekly_resets_at INTEGER);",
        )
        .unwrap();
        assert!(!table_columns(&conn).unwrap().contains("spend_cents"));

        add_missing_columns(&conn).unwrap();
        let cols = table_columns(&conn).unwrap();
        assert!(cols.contains("spend_cents"), "spend_cents added");
        assert!(
            cols.contains("spend_limit_cents"),
            "spend_limit_cents added"
        );
        assert!(
            cols.contains("spend_remaining_cents"),
            "spend_remaining_cents added"
        );
        assert!(cols.contains("spend_resets_at"), "spend_resets_at added");
        assert!(cols.contains("raw_excerpt"), "raw_excerpt added");
        assert!(cols.contains("debug_json"), "debug_json added");

        // Idempotent: a second pass on an already-migrated DB is a no-op.
        add_missing_columns(&conn).unwrap();
    }

    #[test]
    fn insert_select_roundtrip() {
        let store = UsageStore::open_in_memory().unwrap();
        let snap = make_snapshot(1_000_000_000_000);
        let id = store.insert_snapshot(&snap).unwrap();
        assert!(id > 0);
        let got = store.get_latest("claude", None).unwrap().unwrap();
        assert_eq!(got.id, id);
        assert_eq!(got.session_pct, Some(42.0));
        assert_eq!(got.weekly_pct, Some(10.0));
    }

    #[test]
    fn history_returns_newest_first() {
        let store = UsageStore::open_in_memory().unwrap();
        for ms in [1_000_000_000_000_i64, 1_000_000_500_000, 1_000_001_000_000] {
            store.insert_snapshot(&make_snapshot(ms)).unwrap();
        }
        let hist = store.get_history("claude", 0, 10, None).unwrap();
        assert_eq!(hist.len(), 3);
        assert!(hist[0].captured_at > hist[1].captured_at);
        assert!(hist[1].captured_at > hist[2].captured_at);
    }

    #[test]
    fn daily_rollup_arithmetic_on_three_days() {
        let store = UsageStore::open_in_memory().unwrap();
        let day_a = 1_700_000_000_000_i64;
        let day_b = day_a + 86_400_000;
        let day_c = day_a + 2 * 86_400_000;
        let mut snaps = Vec::new();
        for &ms in &[day_a, day_a + 1000, day_b, day_c, day_c + 5000] {
            snaps.push(make_snapshot(ms));
        }
        snaps[0].session_pct = Some(20.0);
        snaps[1].session_pct = Some(40.0);
        snaps[2].session_pct = Some(50.0);
        snaps[3].session_pct = Some(10.0);
        snaps[4].session_pct = Some(70.0);
        for s in &snaps {
            store.insert_snapshot(s).unwrap();
        }
        let roll = store.get_daily_rollup("claude", 0).unwrap();
        assert_eq!(roll.len(), 3, "three distinct days");
        let day0 = &roll[0];
        assert_eq!(day0.avg_session_pct, Some(30.0), "avg of 20+40");
        assert_eq!(day0.max_session_pct, Some(40.0));
        let day2 = &roll[2];
        assert_eq!(day2.max_session_pct, Some(70.0));
    }

    #[test]
    fn get_latest_skips_parse_failures() {
        let store = UsageStore::open_in_memory().unwrap();
        let good = make_snapshot(1_000_000_000_000);
        store.insert_snapshot(&good).unwrap();
        store
            .insert_failure("claude", None, Some("raw"), "regex panic", Some("/usage"))
            .unwrap();
        let latest = store.get_latest("claude", None).unwrap().unwrap();
        assert!(latest.parsed_ok, "get_latest must skip failed snapshots");
        assert_eq!(latest.session_pct, Some(42.0));
        let hist = store.get_history("claude", 0, 10, None).unwrap();
        assert_eq!(hist.len(), 2, "history still contains the failure row");
        assert_eq!(
            hist[0].failure_reason.as_deref(),
            Some("PARSE_ERROR: regex panic")
        );
        assert_eq!(hist[0].probe_command.as_deref(), Some("/usage"));
        assert_eq!(hist[0].raw_excerpt.as_deref(), Some("raw"));
        assert!(hist[0]
            .debug_json
            .as_deref()
            .unwrap_or("")
            .contains("regex panic"));
    }

    #[test]
    fn codex_zero_zero_with_backward_resets_is_rejected() {
        let store = UsageStore::open_in_memory().unwrap();
        let prev = codex_snapshot(
            1_783_601_000_000,
            6.0,
            9.0,
            1_783_609_680_000,
            1_784_139_960_000,
        );
        store.insert_snapshot(&prev).unwrap();

        let bad = codex_snapshot(
            1_783_601_095_000,
            0.0,
            0.0,
            1_783_608_240_000,
            1_784_132_760_000,
        );
        store.insert_snapshot(&bad).unwrap();

        let latest = store.get_latest("codex", None).unwrap().unwrap();
        assert_eq!(latest.session_pct, Some(6.0));
        let hist = store.get_history("codex", 0, 10, None).unwrap();
        assert!(!hist[0].parsed_ok);
        assert_eq!(hist[0].session_pct, Some(0.0));
        assert_eq!(hist[0].weekly_pct, Some(0.0));
        assert!(hist[0]
            .failure_reason
            .as_deref()
            .unwrap_or("")
            .contains("0%/0%"));
        assert!(hist[0]
            .raw_excerpt
            .as_deref()
            .unwrap_or("")
            .contains("OpenAI Codex"));
    }

    #[test]
    fn codex_hundred_hundred_with_identical_resets_is_rejected() {
        let store = UsageStore::open_in_memory().unwrap();
        let prev = codex_snapshot(
            1_783_601_000_000,
            6.0,
            9.0,
            1_783_609_680_000,
            1_784_139_960_000,
        );
        store.insert_snapshot(&prev).unwrap();

        let bad = codex_snapshot(
            1_783_601_095_000,
            100.0,
            100.0,
            1_783_700_000_000,
            1_783_700_000_000,
        );
        store.insert_snapshot(&bad).unwrap();

        let hist = store.get_history("codex", 0, 10, None).unwrap();
        assert!(!hist[0].parsed_ok);
        assert!(hist[0]
            .failure_reason
            .as_deref()
            .unwrap_or("")
            .contains("100%/100%"));
    }

    /// The failure mode that cost a day: the gauge kept rendering its last good
    /// reading and nothing anywhere said why it had stopped moving. The newest
    /// failure now rides on the row the panel already draws.
    #[test]
    fn a_frozen_reading_carries_the_failure_that_froze_it() {
        let store = UsageStore::open_in_memory().unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        store
            .insert_snapshot(&codex_snapshot(now - 86_400_000, 12.0, 7.0, 0, 0))
            .unwrap();
        store
            .insert_failure_kind(
                "codex",
                None,
                None,
                UsageFailureKind::CapabilityDenied,
                "plugin denied readFile",
                None,
            )
            .unwrap();

        let row = store
            .get_latest_with_stale_reason("codex", None)
            .unwrap()
            .expect("the last good reading is still what the panel renders");
        assert!(row.parsed_ok, "the rendered row is still the good reading");
        assert_eq!(row.weekly_pct, Some(7.0));
        let reason = row
            .failure_reason
            .as_deref()
            .expect("staleness must name its cause");
        assert!(
            reason.starts_with(UsageFailureKind::CapabilityDenied.as_str()),
            "the cause is carried as the kind constant, got {reason}"
        );
    }

    #[test]
    fn a_reading_newer_than_the_last_failure_is_not_marked_stale() {
        let store = UsageStore::open_in_memory().unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        store
            .insert_failure_kind(
                "codex",
                None,
                None,
                UsageFailureKind::CapabilityDenied,
                "plugin denied readFile",
                None,
            )
            .unwrap();
        store
            .insert_snapshot(&codex_snapshot(now + 3_600_000, 12.0, 7.0, 0, 0))
            .unwrap();

        let row = store
            .get_latest_with_stale_reason("codex", None)
            .unwrap()
            .unwrap();
        assert_eq!(
            row.failure_reason, None,
            "a recovered probe must not keep flagging a cause that has passed"
        );
    }

    #[test]
    fn codex_zero_after_supported_reset_is_accepted() {
        let store = UsageStore::open_in_memory().unwrap();
        let prev = codex_snapshot(
            1_783_601_000_000,
            98.0,
            9.0,
            1_783_609_680_000,
            1_784_139_960_000,
        );
        store.insert_snapshot(&prev).unwrap();

        let reset = codex_snapshot(
            1_783_610_000_000,
            0.0,
            9.0,
            1_783_628_000_000,
            1_784_139_960_000,
        );
        store.insert_snapshot(&reset).unwrap();

        let latest = store.get_latest("codex", None).unwrap().unwrap();
        assert!(latest.parsed_ok);
        assert_eq!(latest.session_pct, Some(0.0));
        assert_eq!(latest.weekly_pct, Some(9.0));
    }

    #[test]
    fn codex_exhaustion_with_distinct_resets_is_accepted() {
        let store = UsageStore::open_in_memory().unwrap();
        let prev = codex_snapshot(
            1_783_601_000_000,
            98.0,
            99.0,
            1_783_609_680_000,
            1_784_139_960_000,
        );
        store.insert_snapshot(&prev).unwrap();

        let full = codex_snapshot(
            1_783_601_095_000,
            100.0,
            100.0,
            1_783_609_680_000,
            1_784_139_960_000,
        );
        store.insert_snapshot(&full).unwrap();

        let latest = store.get_latest("codex", None).unwrap().unwrap();
        assert!(latest.parsed_ok);
        assert_eq!(latest.session_pct, Some(100.0));
        assert_eq!(latest.weekly_pct, Some(100.0));
    }

    #[test]
    fn failure_taxonomy_is_persisted_in_reason_prefix() {
        let store = UsageStore::open_in_memory().unwrap();
        for (kind, detail) in [
            (UsageFailureKind::Timeout, "agent not ready"),
            (UsageFailureKind::FetchError, "request timed out"),
            (UsageFailureKind::ParseError, "bad frame"),
            (UsageFailureKind::Skip, "empty usage frame"),
        ] {
            store
                .insert_failure_kind("cursor", None, Some("raw"), kind, detail, Some("/usage"))
                .unwrap();
        }

        let hist = store.get_history("cursor", 0, 10, None).unwrap();
        let mut reasons: Vec<_> = hist
            .iter()
            .filter_map(|row| row.failure_reason.as_deref())
            .collect();
        reasons.sort_unstable();
        assert_eq!(
            reasons,
            vec![
                "FETCH_ERROR: request timed out",
                "PARSE_ERROR: bad frame",
                "SKIP: empty usage frame",
                "TIMEOUT: agent not ready"
            ]
        );
    }

    #[test]
    fn spend_fields_roundtrip() {
        let store = UsageStore::open_in_memory().unwrap();
        let mut s = UsageSnapshot::empty("cursor", "raw", 1_000_000_000_000);
        s.spend_cents = Some(1743.5);
        s.spend_limit_cents = Some(2000.0);
        s.spend_remaining_cents = Some(256.5);
        s.spend_resets_at_ms = Some(1_785_542_400_000);
        let id = store.insert_snapshot(&s).unwrap();
        assert!(id > 0);
        let got = store.get_latest("cursor", None).unwrap().unwrap();
        assert_eq!(got.spend_cents, Some(1743.5));
        assert_eq!(got.spend_limit_cents, Some(2000.0));
        assert_eq!(got.spend_remaining_cents, Some(256.5));
        assert_eq!(got.spend_resets_at, Some(1_785_542_400_000));
        assert_eq!(got.session_pct, None);
    }

    #[test]
    fn account_id_filter_isolation() {
        let store = UsageStore::open_in_memory().unwrap();
        let mut s1 = make_snapshot(1_000_000_000_000);
        s1.account_id = Some("acct-a".into());
        let mut s2 = make_snapshot(1_000_000_001_000);
        s2.account_id = Some("acct-b".into());
        store.insert_snapshot(&s1).unwrap();
        store.insert_snapshot(&s2).unwrap();
        let a = store.get_latest("claude", Some("acct-a")).unwrap().unwrap();
        assert_eq!(a.account_id.as_deref(), Some("acct-a"));
        let any = store.get_history("claude", 0, 10, None).unwrap();
        assert_eq!(any.len(), 2);
    }

    #[test]
    fn trim_to_cap_keeps_only_last_n() {
        // Small cap so pruning is exercised in <1s: inserting 10k+ rows one tx
        // each (the real cap) took ~70s and dominated the whole suite. The trim
        // logic is cap-agnostic, so a 50-row cap proves the same behavior.
        const CAP: i64 = 50;
        let store = UsageStore::open_in_memory_with_cap(CAP).unwrap();
        for i in 0..(CAP + 25) {
            store
                .insert_snapshot(&make_snapshot(1_000_000_000_000 + i * 1000))
                .unwrap();
        }
        let count: i64 = store.with_conn(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM usage_snapshots WHERE provider='claude'",
                [],
                |r| r.get(0),
            )
            .unwrap()
        });
        assert_eq!(count, CAP, "rows beyond cap are pruned");
        let latest = store.get_latest("claude", None).unwrap().unwrap();
        let expected_latest = 1_000_000_000_000 + (CAP + 24) * 1000;
        assert_eq!(latest.captured_at, expected_latest, "newest row survives");
    }

    #[test]
    fn prune_older_than_removes_old_rows() {
        let store = UsageStore::open_in_memory().unwrap();
        let s1 = make_snapshot(1_000_000_000_000);
        let s2 = make_snapshot(2_000_000_000_000);
        store.insert_snapshot(&s1).unwrap();
        store.insert_snapshot(&s2).unwrap();
        let n = store.prune_older_than(1_500_000_000_000).unwrap();
        assert_eq!(n, 1);
        let hist = store.get_history("claude", 0, 10, None).unwrap();
        assert_eq!(hist.len(), 1);
    }
}
