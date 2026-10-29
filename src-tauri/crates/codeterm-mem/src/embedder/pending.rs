use rusqlite::{params, Connection};

pub(crate) fn init(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS mem_vector_pending (
            mem_id TEXT PRIMARY KEY,
            reason TEXT NOT NULL,
            queued_at TEXT NOT NULL
        )",
    )
    .map_err(|e| format!("create mem_vector_pending: {e}"))
}

pub fn mark(conn: &Connection, mem_id: &str, reason: &str) -> Result<(), String> {
    init(conn)?;
    conn.execute(
        "INSERT INTO mem_vector_pending (mem_id, reason, queued_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(mem_id) DO UPDATE SET reason = excluded.reason",
        params![mem_id, reason, chrono::Utc::now().to_rfc3339()],
    )
    .map_err(|e| format!("mark {mem_id} vector pending: {e}"))?;
    Ok(())
}

pub fn clear(conn: &Connection, mem_id: &str) -> Result<(), String> {
    init(conn)?;
    conn.execute(
        "DELETE FROM mem_vector_pending WHERE mem_id = ?1",
        params![mem_id],
    )
    .map_err(|e| format!("clear {mem_id} vector pending: {e}"))?;
    Ok(())
}

pub(super) fn clear_all(conn: &Connection) -> Result<(), String> {
    conn.execute("DELETE FROM mem_vector_pending", [])
        .map_err(|e| format!("clear vector pending: {e}"))?;
    Ok(())
}

pub fn count(conn: &Connection) -> u64 {
    conn.query_row("SELECT COUNT(*) FROM mem_vector_pending", [], |r| {
        r.get::<_, i64>(0)
    })
    .map(|count| count as u64)
    .unwrap_or(0)
}

pub fn ids(conn: &Connection) -> Result<Vec<String>, String> {
    init(conn)?;
    let mut stmt = conn
        .prepare("SELECT mem_id FROM mem_vector_pending ORDER BY queued_at, mem_id")
        .map_err(|e| format!("prepare vector pending: {e}"))?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| format!("read vector pending: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("read vector pending: {e}"))
}
