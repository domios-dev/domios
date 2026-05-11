use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::agent::usage_parser::{BreakdownDimension, UsageBreakdownRow, UsageSnapshot};

const SCHEMA: &str = include_str!("schema.sql");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSnapshotRow {
    pub id: i64,
    pub provider: String,
    pub account_id: Option<String>,
    pub captured_at: i64,
    pub parsed_ok: bool,
    pub parse_error: Option<String>,
    pub raw_text: Option<String>,
    pub session_pct: Option<f32>,
    pub session_resets_at: Option<i64>,
    pub weekly_pct: Option<f32>,
    pub weekly_resets_at: Option<i64>,
    pub weekly_sonnet_pct: Option<f32>,
    pub weekly_sonnet_resets_at: Option<i64>,
    pub session_cost_usd: Option<f32>,
    pub session_api_duration_s: Option<i64>,
    pub session_wall_duration_s: Option<i64>,
    pub session_lines_added: Option<i64>,
    pub session_lines_removed: Option<i64>,
    pub session_tokens_input: Option<i64>,
    pub session_tokens_output: Option<i64>,
    pub session_tokens_cache_read: Option<i64>,
    pub session_tokens_cache_write: Option<i64>,
    pub breakdown: Vec<UsageBreakdownRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyRollup {
    pub day: String,
    pub avg_session_pct: Option<f32>,
    pub max_session_pct: Option<f32>,
    pub max_weekly_pct: Option<f32>,
    pub total_tokens: Option<i64>,
}

pub struct UsageStore {
    conn: Mutex<Connection>,
    db_path: PathBuf,
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
        conn.execute_batch(SCHEMA).map_err(|e| format!("init usage schema: {}", e))?;
        Ok(Arc::new(Self {
            conn: Mutex::new(conn),
            db_path,
        }))
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<UsageStoreHandle, String> {
        let conn = Connection::open_in_memory().map_err(|e| format!("open :memory:: {}", e))?;
        conn.execute_batch(SCHEMA).map_err(|e| format!("init usage schema: {}", e))?;
        Ok(Arc::new(Self {
            conn: Mutex::new(conn),
            db_path: PathBuf::from(":memory:"),
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
        let id = insert_header(&tx, snapshot, true, None)?;
        tx.commit().map_err(|e| format!("commit: {}", e))?;
        Ok(id)
    }

    pub fn insert_failure(
        &self,
        provider: &str,
        account_id: Option<&str>,
        raw_text: Option<&str>,
        error: &str,
    ) -> Result<i64, String> {
        let conn = self.conn.lock();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let res = conn.execute(
            r#"INSERT INTO usage_snapshots
               (provider, account_id, captured_at, parsed_ok, parse_error, raw_text)
               VALUES (?1, ?2, ?3, 0, ?4, ?5)"#,
            params![provider, account_id, now_ms, error, raw_text],
        );
        res.map_err(|e| format!("insert failure: {}", e))?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_latest(
        &self,
        provider: &str,
        account_id: Option<&str>,
    ) -> Result<Option<UsageSnapshotRow>, String> {
        let conn = self.conn.lock();
        let sql = format!(
            "{} WHERE provider = ?1 AND (?2 IS NULL OR account_id = ?2) ORDER BY captured_at DESC LIMIT 1",
            SELECT_HEADER
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| format!("prepare get_latest: {}", e))?;
        let row = stmt
            .query_row(params![provider, account_id], row_to_header)
            .ok();
        if let Some(mut header) = row {
            header.breakdown = fetch_breakdown(&conn, header.id)?;
            Ok(Some(header))
        } else {
            Ok(None)
        }
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
        let mut stmt = conn.prepare(&sql).map_err(|e| format!("prepare get_history: {}", e))?;
        let rows: Vec<UsageSnapshotRow> = stmt
            .query_map(params![provider, since_ms, account_id, limit], row_to_header)
            .map_err(|e| format!("query history: {}", e))?
            .filter_map(Result::ok)
            .collect();
        let mut out = Vec::with_capacity(rows.len());
        for mut r in rows {
            r.breakdown = fetch_breakdown(&conn, r.id)?;
            out.push(r);
        }
        Ok(out)
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
                          AVG(session_pct), MAX(session_pct), MAX(weekly_pct),
                          SUM(COALESCE(session_tokens_input,0) + COALESCE(session_tokens_output,0))
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
                    total_tokens: r.get::<_, Option<i64>>(4)?,
                })
            })
            .map_err(|e| format!("query rollup: {}", e))?
            .filter_map(Result::ok)
            .collect();
        Ok(out)
    }

    pub fn get_breakdown_topn(
        &self,
        _provider: &str,
        _dimension: BreakdownDimension,
        _since_ms: i64,
        _n: i64,
    ) -> Result<Vec<(String, f32)>, String> {
        Ok(Vec::new())
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

const SELECT_HEADER: &str =r#"SELECT id, provider, account_id, captured_at, parsed_ok, parse_error, raw_text,
        session_pct, session_resets_at,
        weekly_pct, weekly_resets_at,
        weekly_sonnet_pct, weekly_sonnet_resets_at,
        session_cost_usd, session_api_duration_s, session_wall_duration_s,
        session_lines_added, session_lines_removed,
        session_tokens_input, session_tokens_output,
        session_tokens_cache_read, session_tokens_cache_write
        FROM usage_snapshots"#;

fn row_to_header(r: &rusqlite::Row<'_>) -> rusqlite::Result<UsageSnapshotRow> {
    Ok(UsageSnapshotRow {
        id: r.get(0)?,
        provider: r.get(1)?,
        account_id: r.get(2)?,
        captured_at: r.get(3)?,
        parsed_ok: r.get::<_, i64>(4)? != 0,
        parse_error: r.get(5)?,
        raw_text: r.get(6)?,
        session_pct: r.get::<_, Option<f64>>(7)?.map(|v| v as f32),
        session_resets_at: r.get(8)?,
        weekly_pct: r.get::<_, Option<f64>>(9)?.map(|v| v as f32),
        weekly_resets_at: r.get(10)?,
        weekly_sonnet_pct: r.get::<_, Option<f64>>(11)?.map(|v| v as f32),
        weekly_sonnet_resets_at: r.get(12)?,
        session_cost_usd: r.get::<_, Option<f64>>(13)?.map(|v| v as f32),
        session_api_duration_s: r.get(14)?,
        session_wall_duration_s: r.get(15)?,
        session_lines_added: r.get(16)?,
        session_lines_removed: r.get(17)?,
        session_tokens_input: r.get(18)?,
        session_tokens_output: r.get(19)?,
        session_tokens_cache_read: r.get(20)?,
        session_tokens_cache_write: r.get(21)?,
        breakdown: Vec::new(),
    })
}

fn insert_header(
    tx: &rusqlite::Transaction<'_>,
    snap: &UsageSnapshot,
    parsed_ok: bool,
    parse_error: Option<&str>,
) -> Result<i64, String> {
    tx.execute(
        r#"INSERT INTO usage_snapshots
           (provider, account_id, captured_at, parsed_ok, parse_error, raw_text,
            session_pct, session_resets_at, weekly_pct, weekly_resets_at,
            weekly_sonnet_pct, weekly_sonnet_resets_at,
            session_cost_usd, session_api_duration_s, session_wall_duration_s,
            session_lines_added, session_lines_removed,
            session_tokens_input, session_tokens_output,
            session_tokens_cache_read, session_tokens_cache_write)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6,
                   ?7, ?8, ?9, ?10, ?11, ?12,
                   ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)"#,
        params![
            &snap.provider,
            &snap.account_id,
            snap.captured_at_ms,
            if parsed_ok { 1i64 } else { 0i64 },
            parse_error,
            if parsed_ok { "" } else { snap.raw_text.as_str() },
            snap.session_pct.map(|v| v as f64),
            snap.session_resets_at_ms,
            snap.weekly_pct.map(|v| v as f64),
            snap.weekly_resets_at_ms,
            snap.weekly_sonnet_pct.map(|v| v as f64),
            snap.weekly_sonnet_resets_at_ms,
            snap.session_cost_usd.map(|v| v as f64),
            snap.session_api_duration_s,
            snap.session_wall_duration_s,
            snap.session_lines_added,
            snap.session_lines_removed,
            snap.session_tokens_input,
            snap.session_tokens_output,
            snap.session_tokens_cache_read,
            snap.session_tokens_cache_write,
        ],
    )
    .map_err(|e| format!("insert snapshot: {}", e))?;
    Ok(tx.last_insert_rowid())
}

fn fetch_breakdown(_conn: &Connection, _snapshot_id: i64) -> Result<Vec<UsageBreakdownRow>, String> {
    Ok(Vec::new())
}

fn dim_as_str(d: BreakdownDimension) -> &'static str {
    match d {
        BreakdownDimension::Skill => "skill",
        BreakdownDimension::Subagent => "subagent",
        BreakdownDimension::Plugin => "plugin",
        BreakdownDimension::Heuristic => "heuristic",
    }
}

fn dim_from_str(s: &str) -> BreakdownDimension {
    match s {
        "skill" => BreakdownDimension::Skill,
        "subagent" => BreakdownDimension::Subagent,
        "plugin" => BreakdownDimension::Plugin,
        _ => BreakdownDimension::Heuristic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_snapshot(ms: i64) -> UsageSnapshot {
        let mut s = UsageSnapshot::empty("claude", "raw", ms);
        s.session_pct = Some(42.0);
        s.weekly_pct = Some(10.0);
        s.session_tokens_input = Some(1000);
        s.session_tokens_output = Some(500);
        s.breakdown = vec![
            UsageBreakdownRow {
                dimension: BreakdownDimension::Subagent,
                key: "backend-dev".into(),
                pct: 16.0,
            },
            UsageBreakdownRow {
                dimension: BreakdownDimension::Plugin,
                key: "superpowers".into(),
                pct: 9.0,
            },
        ];
        s
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
        assert_eq!(got.breakdown.len(), 2);
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
    fn breakdown_join_and_cascade_delete() {
        let store = UsageStore::open_in_memory().unwrap();
        let id = store.insert_snapshot(&make_snapshot(2_000_000_000_000)).unwrap();
        let top = store
            .get_breakdown_topn("claude", BreakdownDimension::Subagent, 0, 10)
            .unwrap();
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].0, "backend-dev");
        assert_eq!(top[0].1, 16.0);

        store.with_conn(|c| {
            c.execute("DELETE FROM usage_snapshots WHERE id = ?1", params![id]).unwrap();
        });

        let after: i64 = store.with_conn(|c| {
            c.query_row("SELECT COUNT(*) FROM usage_breakdown", [], |r| r.get(0))
                .unwrap()
        });
        assert_eq!(after, 0, "cascade delete removed breakdown rows");
    }

    #[test]
    fn insert_failure_records_parse_error() {
        let store = UsageStore::open_in_memory().unwrap();
        let id = store
            .insert_failure("claude", None, Some("raw"), "regex panic")
            .unwrap();
        let latest = store.get_latest("claude", None).unwrap().unwrap();
        assert_eq!(latest.id, id);
        assert!(!latest.parsed_ok);
        assert_eq!(latest.parse_error.as_deref(), Some("regex panic"));
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
