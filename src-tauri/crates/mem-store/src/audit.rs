//! Audit-log writers for destructive graph operations.

use crate::error::MemStoreResult;
use rusqlite::Transaction;

pub fn record_purge(
    tx: &Transaction,
    count: u64,
    before: Option<&str>,
    reason_filter: Option<&str>,
    actor: &str,
) -> MemStoreResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    tx.execute(
        "INSERT INTO graph_purge_log (purged_at, count, scope, before, reason_filter, actor, details_json)
         VALUES (?1, ?2, NULL, ?3, ?4, ?5, '{}')",
        rusqlite::params![now, count as i64, before, reason_filter, actor],
    )?;
    Ok(())
}
