//! `insert` does not check the feature flag itself; callers must consult
//! [`kpi_ledger_enabled`] first.

use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

const CURRENT_KPI_SCHEMA_VERSION: u32 = 3;

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
    pub tab_id: String,
    pub role: Option<String>,
    pub provider: String,
    pub model: Option<String>,
    pub timestamp: u64,
    pub kind: KpiEventKind,
    pub bytes: u64,
    /// Only `TokenUsage` events carry this; every other kind leaves it `None`.
    pub tokens: Option<TokenCounts>,
    /// Stable per-event identity (e.g. a report id, or `task_id:round:verdict`) that
    /// makes a retried choke-point call a no-op instead of a second row. `None` for
    /// kinds with no natural retry-safe identity yet.
    pub dedup_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KpiEventRecord {
    pub id: i64,
    pub schema_version: u32,
    pub plan_id: Option<String>,
    pub task_id: Option<String>,
    pub tab_id: String,
    pub role: Option<String>,
    pub provider: String,
    pub model: Option<String>,
    pub timestamp: u64,
    pub kind: KpiEventKind,
    pub bytes: u64,
    pub tokens: Option<TokenCounts>,
    pub dedup_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KindAggregate {
    pub kind: KpiEventKind,
    pub count: u64,
    pub total_bytes: u64,
    pub p50_bytes: u64,
    pub p95_bytes: u64,
}

/// `plan_id: None` is the unattributable/workspace-scope bucket. Only `token_usage`
/// rows carry token columns, so `turns` counts the rows that reported a usage record.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlanAggregate {
    pub plan_id: Option<String>,
    pub count: u64,
    pub total_bytes: u64,
    pub turns: u64,
    pub input_tokens: u64,
    pub uncached_input_tokens: u64,
    pub output_tokens: u64,
}

/// One `(provider, model)` bucket of machine-wide `token_usage` rows. `model: None`
/// is the ledger's own unresolved identity, kept as its own bucket rather than
/// folded into a neighbour or guessed from the provider.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProviderModelUsage {
    pub provider: String,
    pub model: Option<String>,
    pub turns: u64,
    pub input_tokens: u64,
    pub uncached_input_tokens: u64,
    pub output_tokens: u64,
}

impl ProviderModelUsage {
    pub fn measured_tokens(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }
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
                2 => {
                    Self::add_v3_dedup_and_cursors(conn)?;
                    version = 3;
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
                tab_id        TEXT NOT NULL,
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

    /// Partial index: `dedup_key` stays `NULL` (and unindexed) for kinds with no
    /// retry-safe identity yet, so only callers that opt in pay the uniqueness cost.
    /// `kpi_usage_cursors` persists the forward-only usage/compaction read position
    /// across daemon restarts — without it a restart re-reads each session file from
    /// byte/rowid zero and re-inserts every already-recorded row.
    fn add_v3_dedup_and_cursors(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            r#"
            ALTER TABLE kpi_events ADD COLUMN dedup_key TEXT;

            CREATE UNIQUE INDEX IF NOT EXISTS idx_kpi_events_dedup
                ON kpi_events(dedup_key) WHERE dedup_key IS NOT NULL;

            CREATE TABLE IF NOT EXISTS kpi_usage_cursors (
                source_path   TEXT PRIMARY KEY,
                cursor_offset INTEGER NOT NULL
            );

            PRAGMA user_version = 3;
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

    /// `OR IGNORE` only ever fires for rows that set `dedup_key` and collide with one
    /// already committed — a retried choke-point call becomes a silent no-op instead
    /// of a second row. Kinds that leave `dedup_key` `None` insert unconditionally,
    /// same as before this column existed.
    fn insert_with(conn: &Connection, event: &KpiEventInput) -> Result<(), String> {
        conn.execute(
            r#"
            INSERT OR IGNORE INTO kpi_events (
                schema_version, plan_id, task_id, tab_id, role, provider, model,
                ts, kind, bytes, input_tokens, uncached_input_tokens, output_tokens,
                dedup_key
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
            "#,
            params![
                CURRENT_KPI_SCHEMA_VERSION,
                event.plan_id,
                event.task_id,
                event.tab_id,
                event.role,
                event.provider,
                event.model,
                event.timestamp as i64,
                event.kind.label(),
                event.bytes as i64,
                event.tokens.map(|t| t.input as i64),
                event.tokens.map(|t| t.uncached_input as i64),
                event.tokens.map(|t| t.output as i64),
                event.dedup_key,
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

    /// Usage/compaction ingestion's crash-safe counterpart to `insert_many`: the batch
    /// and the advanced read cursor commit in the same transaction, so a crash between
    /// the two is impossible — either both land or neither does, and a restart can
    /// never re-read already-recorded bytes/rows.
    pub fn insert_usage_batch(
        &self,
        events: &[KpiEventInput],
        source_path: &str,
        cursor_offset: u64,
    ) -> Result<(), String> {
        let mut conn = self.conn.lock();
        let tx = conn
            .transaction()
            .map_err(|e| format!("kpi ledger usage transaction: {}", e))?;
        for event in events {
            Self::insert_with(&tx, event)?;
        }
        tx.execute(
            r#"
            INSERT INTO kpi_usage_cursors (source_path, cursor_offset) VALUES (?1, ?2)
            ON CONFLICT(source_path) DO UPDATE SET cursor_offset = excluded.cursor_offset
            "#,
            params![source_path, cursor_offset as i64],
        )
        .map_err(|e| format!("kpi ledger cursor upsert: {}", e))?;
        tx.commit()
            .map_err(|e| format!("kpi ledger usage commit: {}", e))?;
        Ok(())
    }

    /// The persisted forward-only read position for a usage source, or `None` if this
    /// source has never been ingested (a fresh forward-from-zero read).
    pub fn load_usage_cursor(&self, source_path: &str) -> Result<Option<u64>, String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT cursor_offset FROM kpi_usage_cursors WHERE source_path = ?1",
            params![source_path],
            |r| r.get::<_, i64>(0),
        )
        .optional()
        .map_err(|e| format!("kpi ledger cursor load: {}", e))
        .map(|v| v.map(|offset| offset as u64))
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
            SELECT id, schema_version, plan_id, task_id, tab_id, role, provider, model,
                   ts, kind, bytes, input_tokens, uncached_input_tokens, output_tokens, dedup_key
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
            tab_id: r.get(4)?,
            role: r.get(5)?,
            provider: r.get(6)?,
            model: r.get(7)?,
            timestamp: r.get::<_, i64>(8)? as u64,
            kind,
            bytes: r.get::<_, i64>(10)? as u64,
            tokens,
            dedup_key: r.get(14)?,
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

    /// Machine-wide, not plan-scoped: every `token_usage` row in the ledger grouped
    /// by the provider and model recorded on it. `turns` counts the rows themselves,
    /// which for this kind is one row per usage record the provider wrote.
    pub fn aggregate_token_usage_by_provider_model(
        &self,
    ) -> Result<Vec<ProviderModelUsage>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                r#"SELECT provider, model, COUNT(*),
                          COALESCE(SUM(input_tokens), 0),
                          COALESCE(SUM(uncached_input_tokens), 0),
                          COALESCE(SUM(output_tokens), 0)
                   FROM kpi_events
                   WHERE kind = ?1
                   GROUP BY provider, model
                   ORDER BY COALESCE(SUM(input_tokens), 0) + COALESCE(SUM(output_tokens), 0) DESC,
                            provider ASC"#,
            )
            .map_err(|e| format!("kpi ledger provider/model prepare: {}", e))?;
        let rows = stmt
            .query_map(params![KpiEventKind::TokenUsage.label()], |r| {
                Ok(ProviderModelUsage {
                    provider: r.get(0)?,
                    model: r.get::<_, Option<String>>(1)?,
                    turns: r.get::<_, i64>(2)? as u64,
                    input_tokens: r.get::<_, i64>(3)? as u64,
                    uncached_input_tokens: r.get::<_, i64>(4)? as u64,
                    output_tokens: r.get::<_, i64>(5)? as u64,
                })
            })
            .map_err(|e| format!("kpi ledger provider/model query: {}", e))?
            .filter_map(Result::ok)
            .collect();
        Ok(rows)
    }

    pub fn aggregate_by_plan(&self) -> Result<Vec<PlanAggregate>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                // Token rows store their token total in `bytes`; only message payloads are bytes.
                r#"SELECT plan_id, COUNT(*),
                          COALESCE(SUM(CASE WHEN input_tokens IS NULL THEN bytes END), 0),
                          COUNT(input_tokens),
                          COALESCE(SUM(input_tokens), 0),
                          COALESCE(SUM(uncached_input_tokens), 0),
                          COALESCE(SUM(output_tokens), 0)
                   FROM kpi_events GROUP BY plan_id"#,
            )
            .map_err(|e| format!("kpi ledger plan aggregate prepare: {}", e))?;
        let rows = stmt
            .query_map([], |r| {
                Ok(PlanAggregate {
                    plan_id: r.get(0)?,
                    count: r.get::<_, i64>(1)? as u64,
                    total_bytes: r.get::<_, i64>(2)? as u64,
                    turns: r.get::<_, i64>(3)? as u64,
                    input_tokens: r.get::<_, i64>(4)? as u64,
                    uncached_input_tokens: r.get::<_, i64>(5)? as u64,
                    output_tokens: r.get::<_, i64>(6)? as u64,
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
