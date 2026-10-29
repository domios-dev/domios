use crate::MemManager;
use parking_lot::MutexGuard;
use rusqlite::Connection;

impl MemManager {
    /// Returns the WAL checkpoint row, or `(1, -1, -1)` when memory is busy.
    pub fn checkpoint_and_close(&self) -> Result<(i64, i64, i64), String> {
        checkpoint(
            || self.conn.try_lock(),
            || log::warn!("[mem] checkpoint skipped: memory busy"),
        )
    }
}

fn checkpoint<'a>(
    acquire: impl FnOnce() -> Option<MutexGuard<'a, Connection>>,
    on_busy: impl FnOnce(),
) -> Result<(i64, i64, i64), String> {
    let Some(conn) = acquire() else {
        on_busy();
        return Ok((1, -1, -1));
    };
    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })
    .map_err(|error| format!("wal_checkpoint(TRUNCATE) failed: {error}"))
}

#[cfg(test)]
mod tests;
