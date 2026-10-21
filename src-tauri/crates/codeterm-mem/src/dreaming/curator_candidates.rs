use codeterm_blueprint_types::blueprint_memory::curator::dream_candidate;
use rusqlite::Connection;

pub(super) fn render(conn: &Connection, now: i64) -> Result<Option<String>, String> {
    let mut statement = conn.prepare("SELECT id, tags_json FROM mem_items WHERE EXISTS (SELECT 1 FROM json_each(tags_json) WHERE value='curator') ORDER BY id LIMIT 200").map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?;
    let mut ids = Vec::new();
    for row in rows {
        let (id, tags) = row.map_err(|error| error.to_string())?;
        let tags: Vec<String> = serde_json::from_str(&tags).map_err(|error| error.to_string())?;
        let confidence = tags
            .iter()
            .find_map(|tag| tag.strip_prefix("confidence:"))
            .unwrap_or("");
        let observed = tags
            .iter()
            .find_map(|tag| tag.strip_prefix("curator:observed:")?.parse().ok());
        if observed.is_some_and(|at| dream_candidate(true, confidence, at, now)) {
            ids.push(id);
            if ids.len() == 20 {
                break;
            }
        }
    }
    Ok((!ids.is_empty()).then(|| {
        format!(
            "STALE LOW-CONFIDENCE CURATOR CANDIDATES (review; no automatic expiry):\n{}",
            ids.join("\n")
        )
    }))
}
