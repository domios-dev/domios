//! `insert` does not check the feature flag itself; callers must consult
//! [`kpi_ledger_enabled`] first.

use parking_lot::Mutex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

const CURRENT_KPI_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KpiEventKind {
    Send,
    Report,
    Spawn,
    Nudge,
    Heartbeat,
    Poll,
    Bounce,
    Accept,
    TokenUsage,
    Compaction,
}

impl KpiEventKind {
    pub fn label(self) -> &'static str {
        match self {
            KpiEventKind::Send => "send",
            KpiEventKind::Report => "report",
            KpiEventKind::Spawn => "spawn",
            KpiEventKind::Nudge => "nudge",
            KpiEventKind::Heartbeat => "heartbeat",
            KpiEventKind::Poll => "poll",
            KpiEventKind::Bounce => "bounce",
            KpiEventKind::Accept => "accept",
            KpiEventKind::TokenUsage => "token_usage",
            KpiEventKind::Compaction => "compaction",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "send" => Some(KpiEventKind::Send),
            "report" => Some(KpiEventKind::Report),
            "spawn" => Some(KpiEventKind::Spawn),
            "nudge" => Some(KpiEventKind::Nudge),
            "heartbeat" => Some(KpiEventKind::Heartbeat),
            "poll" => Some(KpiEventKind::Poll),
            "bounce" => Some(KpiEventKind::Bounce),
            "accept" => Some(KpiEventKind::Accept),
            "token_usage" => Some(KpiEventKind::TokenUsage),
            "compaction" => Some(KpiEventKind::Compaction),
            _ => None,
        }
    }

    /// Drives per-kind aggregation without a `SELECT DISTINCT kind` round trip.
    pub const ALL: [KpiEventKind; 10] = [
        KpiEventKind::Send,
        KpiEventKind::Report,
        KpiEventKind::Spawn,
        KpiEventKind::Nudge,
        KpiEventKind::Heartbeat,
        KpiEventKind::Poll,
        KpiEventKind::Bounce,
        KpiEventKind::Accept,
        KpiEventKind::TokenUsage,
        KpiEventKind::Compaction,
    ];
}

/// Token breakdown for `token_usage` events. `input` counts every input token the
/// provider billed (cached included); `uncached_input` is the freshly-processed slice.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenCounts {
    pub input: u64,
    pub uncached_input: u64,
    pub output: u64,
}

impl TokenCounts {
    pub fn total(&self) -> u64 {
        self.input.saturating_add(self.output)
    }
}

/// The ledger stamps `schema_version` and `id` at insert time; callers never supply them.
#[derive(Debug, Clone, PartialEq)]
pub struct KpiEventInput {
    pub plan_id: Option<String>,
    pub task_id: Option<String>,
    pub pane_id: String,
    pub role: Option<String>,
    pub provider: String,
    pub model: Option<String>,
    pub timestamp: u64,
    pub kind: KpiEventKind,
    pub bytes: u64,
    /// Only `TokenUsage` events carry this; every other kind leaves it `None`.
    pub tokens: Option<TokenCounts>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KpiEventRecord {
    pub id: i64,
    pub schema_version: u32,
    pub plan_id: Option<String>,
    pub task_id: Option<String>,
    pub pane_id: String,
    pub role: Option<String>,
    pub provider: String,
    pub model: Option<String>,
    pub timestamp: u64,
    pub kind: KpiEventKind,
    pub bytes: u64,
    pub tokens: Option<TokenCounts>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KindAggregate {
    pub kind: KpiEventKind,
    pub count: u64,
    pub total_bytes: u64,
    pub p50_bytes: u64,
    pub p95_bytes: u64,
}

/// `plan_id: None` is the unattributable/workspace-scope bucket.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanAggregate {
    pub plan_id: Option<String>,
    pub count: u64,
    pub total_bytes: u64,
}

pub struct KpiLedger {
    conn: Mutex<Connection>,
}

impl KpiLedger {
    /// Own db file, not `orchestration.db` — schema migrates independently of the report ledger's.
    pub fn open(data_dir: &std::path::Path) -> Result<Self, String> {
        std::fs::create_dir_all(data_dir).map_err(|e| format!("create data dir: {}", e))?;
        Self::open_at(&data_dir.join("kpi_ledger.db"))
    }

    pub fn open_at(db_path: &std::path::Path) -> Result<Self, String> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {}", e))?;
        }
        let conn = Connection::open(db_path).map_err(|e| format!("open kpi_ledger.db: {}", e))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("set busy_timeout: {}", e))?;
        Self::init_schema(&conn).map_err(|e| format!("init kpi ledger schema: {}", e))?;
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
                    Self::add_v2_token_columns(conn)?;
                    version = 2;
                }
                CURRENT_KPI_SCHEMA_VERSION => return Ok(()),
                _ if version > CURRENT_KPI_SCHEMA_VERSION => {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                _ => return Err(rusqlite::Error::InvalidQuery),
            }
        }
    }

    fn create_v1_schema(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS kpi_events (
                id             INTEGER PRIMARY KEY AUTOINCREMENT,
                schema_version INTEGER NOT NULL,
                plan_id        TEXT,
                task_id        TEXT,
                pane_id        TEXT NOT NULL,
                role           TEXT,
                provider       TEXT NOT NULL,
                model          TEXT,
                ts             INTEGER NOT NULL,
                kind           TEXT NOT NULL,
                bytes          INTEGER NOT NULL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_kpi_events_plan ON kpi_events(plan_id);
            CREATE INDEX IF NOT EXISTS idx_kpi_events_kind ON kpi_events(kind);
            CREATE INDEX IF NOT EXISTS idx_kpi_events_ts ON kpi_events(ts);

            PRAGMA user_version = 1;
            "#,
        )
    }

    fn add_v2_token_columns(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            r#"
            ALTER TABLE kpi_events ADD COLUMN input_tokens INTEGER;
            ALTER TABLE kpi_events ADD COLUMN uncached_input_tokens INTEGER;
            ALTER TABLE kpi_events ADD COLUMN output_tokens INTEGER;

            PRAGMA user_version = 2;
            "#,
        )
    }

    fn user_version(conn: &Connection) -> rusqlite::Result<u32> {
        conn.query_row("PRAGMA user_version", [], |r| {
            let version = r.get::<_, i64>(0)?;
            Ok(version as u32)
        })
    }

    /// Write-once: always appends, unlike OrchestrationLedger's UPSERT-on-report.
    pub fn insert(&self, event: &KpiEventInput) -> Result<(), String> {
        let conn = self.conn.lock();
        Self::insert_with(&conn, event)
    }

    fn insert_with(conn: &Connection, event: &KpiEventInput) -> Result<(), String> {
        conn.execute(
            r#"
            INSERT INTO kpi_events (
                schema_version, plan_id, task_id, pane_id, role, provider, model,
                ts, kind, bytes, input_tokens, uncached_input_tokens, output_tokens
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            "#,
            params![
                CURRENT_KPI_SCHEMA_VERSION,
                event.plan_id,
                event.task_id,
                event.pane_id,
                event.role,
                event.provider,
                event.model,
                event.timestamp as i64,
                event.kind.label(),
                event.bytes as i64,
                event.tokens.map(|t| t.input as i64),
                event.tokens.map(|t| t.uncached_input as i64),
                event.tokens.map(|t| t.output as i64),
            ],
        )
        .map_err(|e| format!("kpi ledger insert: {}", e))?;
        Ok(())
    }

    /// One transaction for the whole batch — a burst of coordination events costs one
    /// commit, not one per event.
    pub fn insert_many(&self, events: &[KpiEventInput]) -> Result<(), String> {
        if events.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock();
        let tx = conn
            .transaction()
            .map_err(|e| format!("kpi ledger transaction: {}", e))?;
        for event in events {
            Self::insert_with(&tx, event)?;
        }
        tx.commit()
            .map_err(|e| format!("kpi ledger commit: {}", e))?;
        Ok(())
    }

    /// No pagination — small-scale inspection and tests only.
    pub fn list_all(&self) -> Result<Vec<KpiEventRecord>, String> {
        self.list_where("1 = 1", params![])
    }

    pub fn list_for_plan(&self, plan_id: &str) -> Result<Vec<KpiEventRecord>, String> {
        self.list_where("plan_id = ?1", params![plan_id])
    }

    fn list_where(
        &self,
        predicate: &str,
        query_params: &[&dyn rusqlite::ToSql],
    ) -> Result<Vec<KpiEventRecord>, String> {
        let conn = self.conn.lock();
        let sql = format!(
            r#"
            SELECT id, schema_version, plan_id, task_id, pane_id, role, provider, model,
                   ts, kind, bytes, input_tokens, uncached_input_tokens, output_tokens
            FROM kpi_events
            WHERE {}
            ORDER BY ts ASC, id ASC
            "#,
            predicate
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| format!("kpi ledger list prepare: {}", e))?;
        let rows = stmt
            .query_map(query_params, Self::row_from_sql)
            .map_err(|e| format!("kpi ledger list query: {}", e))?
            .filter_map(Result::ok)
            .collect();
        Ok(rows)
    }

    fn row_from_sql(r: &rusqlite::Row<'_>) -> rusqlite::Result<KpiEventRecord> {
        let kind_label: String = r.get(9)?;
        let kind = KpiEventKind::parse(&kind_label).unwrap_or(KpiEventKind::Send);
        let tokens = r
            .get::<_, Option<i64>>(11)?
            .map(|input| -> rusqlite::Result<TokenCounts> {
                Ok(TokenCounts {
                    input: input as u64,
                    uncached_input: r.get::<_, Option<i64>>(12)?.unwrap_or(0) as u64,
                    output: r.get::<_, Option<i64>>(13)?.unwrap_or(0) as u64,
                })
            })
            .transpose()?;
        Ok(KpiEventRecord {
            id: r.get(0)?,
            schema_version: r.get::<_, i64>(1)? as u32,
            plan_id: r.get(2)?,
            task_id: r.get(3)?,
            pane_id: r.get(4)?,
            role: r.get(5)?,
            provider: r.get(6)?,
            model: r.get(7)?,
            timestamp: r.get::<_, i64>(8)? as u64,
            kind,
            bytes: r.get::<_, i64>(10)? as u64,
            tokens,
        })
    }

    /// Kinds with zero matching events are omitted, not zero-filled.
    pub fn aggregate_by_kind(&self, plan_id: Option<&str>) -> Result<Vec<KindAggregate>, String> {
        let conn = self.conn.lock();
        let mut out = Vec::new();
        for kind in KpiEventKind::ALL {
            let sizes = Self::bytes_for_kind(&conn, kind, plan_id)?;
            if sizes.is_empty() {
                continue;
            }
            let count = sizes.len() as u64;
            let total_bytes: u64 = sizes.iter().sum();
            out.push(KindAggregate {
                kind,
                count,
                total_bytes,
                p50_bytes: percentile(&sizes, 50.0),
                p95_bytes: percentile(&sizes, 95.0),
            });
        }
        Ok(out)
    }

    fn bytes_for_kind(
        conn: &Connection,
        kind: KpiEventKind,
        plan_id: Option<&str>,
    ) -> Result<Vec<u64>, String> {
        let mut stmt = if plan_id.is_some() {
            conn.prepare(
                "SELECT bytes FROM kpi_events WHERE kind = ?1 AND plan_id = ?2 ORDER BY bytes ASC",
            )
        } else {
            conn.prepare("SELECT bytes FROM kpi_events WHERE kind = ?1 ORDER BY bytes ASC")
        }
        .map_err(|e| format!("kpi ledger aggregate prepare: {}", e))?;

        fn read_bytes_col(r: &rusqlite::Row<'_>) -> rusqlite::Result<i64> {
            r.get::<_, i64>(0)
        }
        let rows = if let Some(plan_id) = plan_id {
            stmt.query_map(params![kind.label(), plan_id], read_bytes_col)
        } else {
            stmt.query_map(params![kind.label()], read_bytes_col)
        }
        .map_err(|e| format!("kpi ledger aggregate query: {}", e))?
        .filter_map(Result::ok)
        .map(|b| b as u64)
        .collect();
        Ok(rows)
    }

    pub fn aggregate_by_plan(&self) -> Result<Vec<PlanAggregate>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare("SELECT plan_id, COUNT(*), COALESCE(SUM(bytes), 0) FROM kpi_events GROUP BY plan_id")
            .map_err(|e| format!("kpi ledger plan aggregate prepare: {}", e))?;
        let rows = stmt
            .query_map([], |r| {
                Ok(PlanAggregate {
                    plan_id: r.get(0)?,
                    count: r.get::<_, i64>(1)? as u64,
                    total_bytes: r.get::<_, i64>(2)? as u64,
                })
            })
            .map_err(|e| format!("kpi ledger plan aggregate query: {}", e))?
            .filter_map(Result::ok)
            .collect();
        Ok(rows)
    }
}

/// Nearest-rank percentile; caller must pre-sort ascending. Empty input returns 0.
fn percentile(sorted_ascending: &[u64], p: f64) -> u64 {
    if sorted_ascending.is_empty() {
        return 0;
    }
    let n = sorted_ascending.len();
    let rank = ((p / 100.0) * n as f64).ceil() as usize;
    let idx = rank.saturating_sub(1).min(n - 1);
    sorted_ascending[idx]
}

pub fn kpi_ledger_enabled(config: &crate::config::schema::AppConfig) -> bool {
    config.kpi.enabled
}

#[cfg(test)]
mod tests;
