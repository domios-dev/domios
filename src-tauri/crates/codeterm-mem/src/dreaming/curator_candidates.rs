use crate::solution_metadata::Confidence;
use rusqlite::Connection;

#[cfg(test)]
mod tests;

pub(super) fn render(conn: &Connection, now: i64) -> Result<Option<String>, String> {
    let mut statement = conn.prepare("SELECT s.item_id FROM solution_metadata s JOIN mem_items m ON m.id=s.item_id WHERE m.kind='skill' AND json_extract(s.metadata_json,'$.curator.confidence')='low' AND json_extract(s.metadata_json,'$.curator.observed_at')<=?1 ORDER BY s.item_id LIMIT 20").map_err(|error|error.to_string())?;
    let rows = statement
        .query_map([now.saturating_sub(60 * 86400)], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|error| error.to_string())?;
    let mut ids = Vec::new();
    for row in rows {
        let id = row.map_err(|error| error.to_string())?;
        let metadata = crate::backend::sqlite::blueprint::metadata::read(conn, &id)?
            .ok_or("solution metadata disappeared")?;
        let curator = metadata.curator.ok_or("Curator metadata absent")?;
        if curator.confidence == Confidence::Low
            && now.saturating_sub(curator.observed_at) >= 60 * 86400
        {
            ids.push(id);
        }
    }
    Ok((!ids.is_empty()).then(|| {
        format!(
            "STALE LOW-CONFIDENCE CURATOR CANDIDATES (review; no automatic expiry):\n{}",
            ids.join("\n")
        )
    }))
}
