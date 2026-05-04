use rusqlite::{params, Connection};

use super::PinnedPosition;

pub fn list_pinned_positions(conn: &Connection) -> Result<Vec<PinnedPosition>, String> {
    let mut stmt = conn
        .prepare("SELECT mem_id, x, y FROM graph_pinned_positions ORDER BY updated_at DESC")
        .map_err(|e| format!("prepare list_pinned: {}", e))?;
    let rows = stmt
        .query_map([], |r| Ok(PinnedPosition {
            mem_id: r.get(0)?,
            x: r.get(1)?,
            y: r.get(2)?,
        }))
        .map_err(|e| format!("query list_pinned: {}", e))?;
    rows.filter_map(|r| r.ok()).map(Ok).collect()
}

/// Upsert a pinned position. Returns `Ok(false)` when the mem_id doesn't
/// exist — callers translate that to a 404 instead of creating an orphan
/// row (the FOREIGN KEY would catch it anyway, but explicit is nicer).
pub fn set_pinned_position(
    conn: &Connection,
    mem_id: &str,
    x: f64,
    y: f64,
) -> Result<bool, String> {
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM mem_items WHERE id = ?1",
            params![mem_id],
            |r| r.get(0),
        )
        .map_err(|e| format!("check mem_item: {}", e))?;
    if exists == 0 {
        return Ok(false);
    }
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        r#"INSERT INTO graph_pinned_positions (mem_id, x, y, updated_at)
           VALUES (?1, ?2, ?3, ?4)
           ON CONFLICT(mem_id) DO UPDATE SET x = excluded.x, y = excluded.y, updated_at = excluded.updated_at"#,
        params![mem_id, x, y, now],
    )
    .map_err(|e| format!("upsert pinned: {}", e))?;
    Ok(true)
}

pub fn delete_pinned_position(conn: &Connection, mem_id: &str) -> Result<usize, String> {
    conn.execute(
        "DELETE FROM graph_pinned_positions WHERE mem_id = ?1",
        params![mem_id],
    )
    .map_err(|e| format!("delete pinned: {}", e))
}
