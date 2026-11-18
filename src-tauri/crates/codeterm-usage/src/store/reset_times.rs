use super::UsageSnapshotRow;
use rusqlite::{params, Connection, OptionalExtension};

pub(super) fn apply(conn: &Connection, row: &mut UsageSnapshotRow, now: i64) -> Result<(), String> {
    for (column, value) in [
        ("session_resets_at", &mut row.session_resets_at),
        ("weekly_resets_at", &mut row.weekly_resets_at),
    ] {
        let sql = format!(
            "SELECT {column} FROM usage_snapshots
             WHERE provider = ?1 AND account_id IS ?2 AND parsed_ok = 1
               AND captured_at <= ?3 AND {column} IS NOT NULL
             ORDER BY captured_at DESC, id DESC LIMIT 1"
        );
        *value = conn
            .query_row(
                &sql,
                params![row.provider, row.account_id, row.captured_at],
                |r| r.get::<_, i64>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .filter(|reset| *reset > now);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
