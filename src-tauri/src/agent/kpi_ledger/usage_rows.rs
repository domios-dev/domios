//! Usage ingestion's ledger side: the per-source read watermark and the explicit
//! re-derivation of token rows written before each row named its provider response.

use std::collections::BTreeSet;

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::Serialize;

use super::{KpiEventInput, KpiEventKind, KpiLedger, TokenCounts};

/// Running session totals a provider reports on every usage line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CumulativeUsage {
    pub input: u64,
    pub cached_input: u64,
    pub output: u64,
    pub total: u64,
}

impl CumulativeUsage {
    pub fn as_tokens(self) -> TokenCounts {
        TokenCounts {
            input: self.input,
            uncached_input: self.input.saturating_sub(self.cached_input),
            output: self.output,
        }
    }

    /// `None` when nothing advanced; a counter that went backwards starts a new run from zero.
    pub fn advance_from(self, previous: CumulativeUsage) -> Option<TokenCounts> {
        if self == previous {
            return None;
        }
        let monotonic = self.input >= previous.input
            && self.cached_input >= previous.cached_input
            && self.output >= previous.output
            && self.total >= previous.total;
        if !monotonic {
            return Some(self.as_tokens());
        }
        let input = self.input - previous.input;
        let cached = self.cached_input - previous.cached_input;
        Some(TokenCounts {
            input,
            uncached_input: input.saturating_sub(cached),
            output: self.output - previous.output,
        })
    }
}

/// `cumulative: None` on a persisted row means the watermark predates cumulative tracking.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageCursorState {
    pub offset: u64,
    pub cumulative: Option<CumulativeUsage>,
}

/// One provider response re-derived from its transcript. `observed` holds the
/// `(timestamp, tokens)` each folded line reported, which is how legacy rows were keyed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebuiltResponse {
    pub key: String,
    pub timestamp: u64,
    pub tokens: TokenCounts,
    pub observed: Vec<(u64, TokenCounts)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct UsageRebuildOutcome {
    pub sources: u64,
    pub missing_sources: u64,
    pub responses: u64,
    pub legacy_rows_removed: u64,
    pub unattributed_responses: u64,
    #[serde(skip)]
    pub plans: BTreeSet<String>,
}

impl UsageRebuildOutcome {
    pub fn absorb(&mut self, other: UsageRebuildOutcome) {
        self.sources += other.sources;
        self.missing_sources += other.missing_sources;
        self.responses += other.responses;
        self.legacy_rows_removed += other.legacy_rows_removed;
        self.unattributed_responses += other.unattributed_responses;
        self.plans.extend(other.plans);
    }
}

struct LegacyRow {
    id: i64,
    plan_id: Option<String>,
    task_id: Option<String>,
    tab_id: String,
    role: Option<String>,
    provider: String,
    model: Option<String>,
    workspace_id: Option<String>,
}

impl LegacyRow {
    fn token_event(&self, response: &RebuiltResponse) -> KpiEventInput {
        KpiEventInput {
            plan_id: self.plan_id.clone(),
            task_id: self.task_id.clone(),
            tab_id: self.tab_id.clone(),
            role: self.role.clone(),
            provider: self.provider.clone(),
            model: self.model.clone(),
            timestamp: response.timestamp,
            kind: KpiEventKind::TokenUsage,
            bytes: response.tokens.total(),
            tokens: Some(response.tokens),
            dedup_key: Some(response.key.clone()),
            workspace_id: self.workspace_id.clone(),
            tab_kind: None,
        }
    }
}

impl KpiLedger {
    /// The batch and the advanced cursor commit in one transaction, so a restart never
    /// re-reads recorded bytes and never skips unrecorded ones.
    pub fn insert_usage_batch(
        &self,
        events: &[KpiEventInput],
        source_path: &str,
        cursor: UsageCursorState,
    ) -> Result<(), String> {
        let mut conn = self.conn.lock();
        let tx = conn
            .transaction()
            .map_err(|e| format!("kpi ledger usage transaction: {}", e))?;
        for event in events {
            Self::insert_with(&tx, event)?;
        }
        upsert_cursor(&tx, source_path, cursor)?;
        tx.commit()
            .map_err(|e| format!("kpi ledger usage commit: {}", e))?;
        self.committed();
        Ok(())
    }

    /// `None` if this source has never been ingested (a fresh forward-from-zero read).
    pub fn load_usage_cursor(&self, source_path: &str) -> Result<Option<UsageCursorState>, String> {
        let conn = self.conn.lock();
        conn.query_row(
            &format!("SELECT {CURSOR_COLUMNS} FROM kpi_usage_cursors WHERE source_path = ?1"),
            params![source_path],
            cursor_from_row,
        )
        .optional()
        .map(|row| row.map(|(_, state)| state))
        .map_err(|e| format!("kpi ledger cursor load: {}", e))
    }

    pub fn list_usage_cursors(&self) -> Result<Vec<(String, UsageCursorState)>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {CURSOR_COLUMNS} FROM kpi_usage_cursors ORDER BY source_path ASC"
            ))
            .map_err(|e| format!("kpi ledger cursor list prepare: {}", e))?;
        let rows = stmt
            .query_map([], cursor_from_row)
            .map_err(|e| format!("kpi ledger cursor list: {}", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("kpi ledger cursor row: {}", e))?;
        Ok(rows)
    }

    /// Replaces one source's line-counted rows with one keyed row per response, charged
    /// to the tab and plan the legacy rows carried. Keyed rows only merge, so a repeat
    /// run changes nothing.
    pub fn rebuild_usage_source(
        &self,
        source_path: &str,
        responses: &[RebuiltResponse],
        superseded: &[(u64, TokenCounts)],
        cursor: UsageCursorState,
    ) -> Result<UsageRebuildOutcome, String> {
        let mut conn = self.conn.lock();
        let tx = conn
            .transaction()
            .map_err(|e| format!("kpi ledger rebuild transaction: {}", e))?;
        let mut outcome = UsageRebuildOutcome {
            sources: 1,
            ..Default::default()
        };
        let mut legacy_ids = BTreeSet::new();
        for response in responses {
            outcome.responses += 1;
            let mut owner: Option<LegacyRow> = None;
            for (timestamp, tokens) in &response.observed {
                for row in legacy_rows(&tx, *timestamp, *tokens)? {
                    legacy_ids.insert(row.id);
                    if owner.as_ref().is_none_or(|current| row.id < current.id) {
                        owner = Some(row);
                    }
                }
            }
            match owner {
                Some(row) => {
                    outcome.plans.extend(row.plan_id.clone());
                    Self::insert_with(&tx, &row.token_event(response))?;
                }
                None => {
                    if !merge_existing(&tx, response)? {
                        outcome.unattributed_responses += 1;
                    }
                }
            }
        }
        for (timestamp, tokens) in superseded {
            for row in legacy_rows(&tx, *timestamp, *tokens)? {
                legacy_ids.insert(row.id);
            }
        }
        for id in &legacy_ids {
            tx.execute("DELETE FROM kpi_events WHERE id = ?1", params![id])
                .map_err(|e| format!("kpi ledger rebuild delete: {}", e))?;
        }
        outcome.legacy_rows_removed = legacy_ids.len() as u64;
        upsert_cursor(&tx, source_path, cursor)?;
        tx.commit()
            .map_err(|e| format!("kpi ledger rebuild commit: {}", e))?;
        self.committed();
        Ok(outcome)
    }
}

const CURSOR_COLUMNS: &str = "source_path, cursor_offset, cumulative_input, cumulative_cached, \
                              cumulative_output, cumulative_total";

fn cursor_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<(String, UsageCursorState)> {
    let totals = (
        r.get::<_, Option<i64>>(2)?,
        r.get::<_, Option<i64>>(3)?,
        r.get::<_, Option<i64>>(4)?,
        r.get::<_, Option<i64>>(5)?,
    );
    let cumulative = match totals {
        (Some(input), Some(cached), Some(output), Some(total)) => Some(CumulativeUsage {
            input: input as u64,
            cached_input: cached as u64,
            output: output as u64,
            total: total as u64,
        }),
        _ => None,
    };
    Ok((
        r.get(0)?,
        UsageCursorState {
            offset: r.get::<_, i64>(1)? as u64,
            cumulative,
        },
    ))
}

fn upsert_cursor(
    conn: &Connection,
    source_path: &str,
    cursor: UsageCursorState,
) -> Result<(), String> {
    let totals = cursor.cumulative;
    conn.execute(
        r#"
        INSERT INTO kpi_usage_cursors (
            source_path, cursor_offset,
            cumulative_input, cumulative_cached, cumulative_output, cumulative_total
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ON CONFLICT(source_path) DO UPDATE SET
            cursor_offset = excluded.cursor_offset,
            cumulative_input = excluded.cumulative_input,
            cumulative_cached = excluded.cumulative_cached,
            cumulative_output = excluded.cumulative_output,
            cumulative_total = excluded.cumulative_total
        "#,
        params![
            source_path,
            cursor.offset as i64,
            totals.map(|t| t.input as i64),
            totals.map(|t| t.cached_input as i64),
            totals.map(|t| t.output as i64),
            totals.map(|t| t.total as i64),
        ],
    )
    .map_err(|e| format!("kpi ledger cursor upsert: {}", e))?;
    Ok(())
}

fn legacy_rows(
    tx: &Transaction<'_>,
    timestamp: u64,
    tokens: TokenCounts,
) -> Result<Vec<LegacyRow>, String> {
    let mut stmt = tx
        .prepare(
            r#"SELECT id, plan_id, task_id, tab_id, role, provider, model, workspace_id
               FROM kpi_events
               WHERE kind = ?1 AND dedup_key IS NULL AND ts = ?2
                 AND input_tokens = ?3 AND uncached_input_tokens = ?4 AND output_tokens = ?5
               ORDER BY id ASC"#,
        )
        .map_err(|e| format!("kpi ledger legacy rows prepare: {}", e))?;
    let rows = stmt
        .query_map(
            params![
                KpiEventKind::TokenUsage.label(),
                timestamp as i64,
                tokens.input as i64,
                tokens.uncached_input as i64,
                tokens.output as i64,
            ],
            |r| {
                Ok(LegacyRow {
                    id: r.get(0)?,
                    plan_id: r.get(1)?,
                    task_id: r.get(2)?,
                    tab_id: r.get(3)?,
                    role: r.get(4)?,
                    provider: r.get(5)?,
                    model: r.get(6)?,
                    workspace_id: r.get(7)?,
                })
            },
        )
        .map_err(|e| format!("kpi ledger legacy rows query: {}", e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("kpi ledger legacy row: {}", e))?;
    Ok(rows)
}

/// A response already recorded under its key keeps its own attribution; only its
/// figures can grow.
fn merge_existing(tx: &Transaction<'_>, response: &RebuiltResponse) -> Result<bool, String> {
    let changed = tx
        .execute(
            r#"UPDATE kpi_events SET
                   input_tokens = MAX(COALESCE(input_tokens, 0), ?2),
                   uncached_input_tokens = MAX(COALESCE(uncached_input_tokens, 0), ?3),
                   output_tokens = MAX(COALESCE(output_tokens, 0), ?4),
                   bytes = MAX(COALESCE(input_tokens, 0), ?2) + MAX(COALESCE(output_tokens, 0), ?4)
               WHERE dedup_key = ?1"#,
            params![
                response.key,
                response.tokens.input as i64,
                response.tokens.uncached_input as i64,
                response.tokens.output as i64,
            ],
        )
        .map_err(|e| format!("kpi ledger rebuild merge: {}", e))?;
    Ok(changed > 0)
}
