//! Bounded analytics reads: one grouped pass per bucket set, reused until the
//! ledger commits a newer row.

use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use codeterm_plan::service_plan::state::analytics::{
    BucketBound, LedgerAggregates, LedgerCoverage, TabCreatedAggregateRow, TabCreatedKind,
    TokenAggregateRow,
};
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};

use super::{KpiEventKind, KpiLedger};

pub(super) const WORKSPACE_RECORDED_SINCE: &str = "workspace_recorded_since";
pub(super) const TABS_COUNTED_SINCE: &str = "tabs_counted_since";
const LEDGER_MARKS: [&str; 2] = [WORKSPACE_RECORDED_SINCE, TABS_COUNTED_SINCE];

const CACHE_ENTRIES: usize = 8;
/// Grouped rows one read may return; a range past it is refused, never truncated.
pub const MAX_AGGREGATE_ROWS: usize = 100_000;

#[derive(Default)]
pub(super) struct AggregateCache {
    entries: VecDeque<CacheEntry>,
    reads: u64,
}

struct CacheEntry {
    generation: u64,
    bounds: Vec<(u64, u64)>,
    aggregates: Arc<LedgerAggregates>,
}

impl KpiLedger {
    pub fn analytics_aggregates(
        &self,
        buckets: &[BucketBound],
    ) -> Result<Arc<LedgerAggregates>, String> {
        self.analytics_aggregates_within(buckets, MAX_AGGREGATE_ROWS)
    }

    /// The generation is read before the query, so a row committed mid-read leaves
    /// the entry already stale and the next call reads again.
    fn analytics_aggregates_within(
        &self,
        buckets: &[BucketBound],
        row_limit: usize,
    ) -> Result<Arc<LedgerAggregates>, String> {
        let bounds: Vec<(u64, u64)> = buckets
            .iter()
            .map(|bucket| (bucket.start, bucket.end))
            .collect();
        let generation = self.generation.load(Ordering::SeqCst);
        if let Some(hit) = self.cached_aggregates(generation, &bounds) {
            return Ok(hit);
        }
        let aggregates = Arc::new(self.read_aggregates(&bounds, row_limit)?);
        self.store_aggregates(generation, bounds, Arc::clone(&aggregates));
        Ok(aggregates)
    }

    fn cached_aggregates(
        &self,
        generation: u64,
        bounds: &[(u64, u64)],
    ) -> Option<Arc<LedgerAggregates>> {
        self.analytics_cache
            .lock()
            .entries
            .iter()
            .find(|entry| entry.generation == generation && entry.bounds == bounds)
            .map(|entry| Arc::clone(&entry.aggregates))
    }

    fn store_aggregates(
        &self,
        generation: u64,
        bounds: Vec<(u64, u64)>,
        aggregates: Arc<LedgerAggregates>,
    ) {
        let mut cache = self.analytics_cache.lock();
        cache.reads += 1;
        cache
            .entries
            .retain(|entry| entry.generation == generation && entry.bounds != bounds);
        cache.entries.push_front(CacheEntry {
            generation,
            bounds,
            aggregates,
        });
        cache.entries.truncate(CACHE_ENTRIES);
    }

    fn read_aggregates(
        &self,
        bounds: &[(u64, u64)],
        row_limit: usize,
    ) -> Result<LedgerAggregates, String> {
        let conn = self.conn.lock();
        let coverage = read_coverage(&conn)?;
        if bounds.is_empty() {
            return Ok(LedgerAggregates {
                coverage,
                ..Default::default()
            });
        }
        Ok(LedgerAggregates {
            coverage,
            tokens: read_token_rows(&conn, bounds, row_limit)?,
            tabs: read_tab_rows(&conn, bounds, row_limit)?,
        })
    }

    #[cfg(test)]
    pub(crate) fn analytics_reads(&self) -> u64 {
        self.analytics_cache.lock().reads
    }
}

/// Stamped once, by the migration that starts recording each figure.
pub(super) fn stamp_ledger_marks(conn: &Connection) -> rusqlite::Result<()> {
    for mark in LEDGER_MARKS {
        conn.execute(
            "INSERT OR IGNORE INTO kpi_ledger_marks (mark, since_ts)
             VALUES (?1, CAST(strftime('%s', 'now') AS INTEGER))",
            params![mark],
        )?;
    }
    Ok(())
}

fn read_coverage(conn: &Connection) -> Result<LedgerCoverage, String> {
    let (first, last) = conn
        .query_row("SELECT MIN(ts), MAX(ts) FROM kpi_events", [], |r| {
            Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<i64>>(1)?))
        })
        .map_err(|e| format!("kpi analytics coverage: {e}"))?;
    let mark = |name: &str| -> Result<Option<u64>, String> {
        conn.query_row(
            "SELECT since_ts FROM kpi_ledger_marks WHERE mark = ?1",
            params![name],
            |r| r.get::<_, i64>(0),
        )
        .optional()
        .map(|since| since.map(|ts| ts as u64))
        .map_err(|e| format!("kpi analytics mark {name}: {e}"))
    };
    Ok(LedgerCoverage {
        first_recorded_at: first.map(|ts| ts as u64),
        last_recorded_at: last.map(|ts| ts as u64),
        workspace_recorded_since: mark(WORKSPACE_RECORDED_SINCE)?,
        tabs_counted_since: mark(TABS_COUNTED_SINCE)?,
    })
}

/// Buckets join as a literal table so each row is charged to the one bucket its own
/// timestamp falls in; the `(kind, ts)` index bounds the scan to the range.
fn grouped_query(
    bounds: &[(u64, u64)],
    kind: KpiEventKind,
    select: &str,
    group: &str,
    row_limit: usize,
) -> (String, Vec<Value>) {
    let rows = vec!["(?, ?, ?)"; bounds.len()].join(", ");
    let sql = format!(
        "WITH buckets(idx, start_ts, end_ts) AS (VALUES {rows})
         SELECT buckets.idx, {select}
         FROM buckets
         JOIN kpi_events e ON e.ts >= buckets.start_ts AND e.ts < buckets.end_ts
         WHERE e.kind = ? AND e.ts >= ? AND e.ts < ?
         GROUP BY buckets.idx, {group}
         LIMIT ?"
    );
    let mut values = Vec::with_capacity(bounds.len() * 3 + 4);
    for (index, (start, end)) in bounds.iter().enumerate() {
        values.push(Value::Integer(index as i64));
        values.push(Value::Integer(*start as i64));
        values.push(Value::Integer(*end as i64));
    }
    let range_start = bounds.iter().map(|(start, _)| *start).min().unwrap_or(0);
    let range_end = bounds.iter().map(|(_, end)| *end).max().unwrap_or(0);
    values.push(Value::Text(kind.label().to_string()));
    values.push(Value::Integer(range_start as i64));
    values.push(Value::Integer(range_end as i64));
    values.push(Value::Integer(row_limit as i64 + 1));
    (sql, values)
}

fn within_limit<T>(rows: Vec<T>, row_limit: usize) -> Result<Vec<T>, String> {
    if rows.len() > row_limit {
        return Err(format!(
            "the range holds more than {row_limit} grouped rows; narrow the range"
        ));
    }
    Ok(rows)
}

fn read_token_rows(
    conn: &Connection,
    bounds: &[(u64, u64)],
    row_limit: usize,
) -> Result<Vec<TokenAggregateRow>, String> {
    let (sql, values) = grouped_query(
        bounds,
        KpiEventKind::TokenUsage,
        "e.provider, e.model, e.workspace_id, e.plan_id, COUNT(*),
         COALESCE(SUM(e.input_tokens), 0), COALESCE(SUM(e.uncached_input_tokens), 0),
         COALESCE(SUM(e.output_tokens), 0)",
        "e.provider, e.model, e.workspace_id, e.plan_id",
        row_limit,
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| format!("kpi analytics token prepare: {e}"))?;
    let rows = stmt
        .query_map(params_from_iter(values), |r| {
            Ok(TokenAggregateRow {
                bucket: r.get::<_, i64>(0)? as usize,
                provider: r.get(1)?,
                model: r.get(2)?,
                workspace_id: r.get(3)?,
                plan_id: r.get(4)?,
                turns: r.get::<_, i64>(5)? as u64,
                input_tokens: r.get::<_, i64>(6)? as u64,
                uncached_input_tokens: r.get::<_, i64>(7)? as u64,
                output_tokens: r.get::<_, i64>(8)? as u64,
            })
        })
        .map_err(|e| format!("kpi analytics token query: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("kpi analytics token row: {e}"))?;
    within_limit(rows, row_limit)
}

fn read_tab_rows(
    conn: &Connection,
    bounds: &[(u64, u64)],
    row_limit: usize,
) -> Result<Vec<TabCreatedAggregateRow>, String> {
    let (sql, values) = grouped_query(
        bounds,
        KpiEventKind::TabCreated,
        "e.tab_kind, e.workspace_id, COUNT(*)",
        "e.tab_kind, e.workspace_id",
        row_limit,
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| format!("kpi analytics tab prepare: {e}"))?;
    let rows = stmt
        .query_map(params_from_iter(values), |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })
        .map_err(|e| format!("kpi analytics tab query: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("kpi analytics tab row: {e}"))?;
    Ok(within_limit(rows, row_limit)?
        .into_iter()
        .filter_map(|(bucket, kind, workspace_id, count)| {
            let Some(tab_kind) = kind.as_deref().and_then(TabCreatedKind::parse) else {
                log::warn!("[kpi] {count} tab_created rows with kind {kind:?} were not counted");
                return None;
            };
            Some(TabCreatedAggregateRow {
                bucket: bucket as usize,
                tab_kind,
                workspace_id,
                count: count as u64,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests;
