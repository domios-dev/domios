use rusqlite::Connection;
use crate::error::MemStoreResult;

pub fn run(conn: &Connection) -> MemStoreResult<()> {
    add_reason_column(conn)?;
    create_graph_purge_log(conn)?;
    Ok(())
}

fn add_reason_column(conn: &Connection) -> MemStoreResult<()> {
    let cols: Vec<String> = conn.prepare("PRAGMA table_info(relations)")?
        .query_map([], |r| r.get::<_, String>(1))?
        .filter_map(Result::ok).collect();
    if !cols.iter().any(|c| c == "reason") {
        conn.execute_batch("ALTER TABLE relations ADD COLUMN reason TEXT;")?;
    }
    Ok(())
}

fn create_graph_purge_log(conn: &Connection) -> MemStoreResult<()> {
    conn.execute_batch(
        r#"CREATE TABLE IF NOT EXISTS graph_purge_log (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            purged_at   TEXT NOT NULL,
            count       INTEGER NOT NULL,
            scope       TEXT,
            before      TEXT,
            reason_filter TEXT,
            actor       TEXT NOT NULL,
            details_json TEXT NOT NULL DEFAULT '{}'
        );
        CREATE INDEX IF NOT EXISTS idx_purge_at ON graph_purge_log(purged_at DESC);"#,
    )?;
    Ok(())
}
