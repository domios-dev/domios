use super::*;
use codeterm_blueprint_types::blueprint_document::calls::{
    self, projection::ExpandedGraph, MemoryResolver,
};
use rusqlite::OptionalExtension;

impl MemManager {
    pub fn blueprint_solution_show(&self, area: &str, id: &str) -> Result<ExpandedGraph, String> {
        if area.is_empty() {
            return Err("Saved solution requires a workspace".into());
        }
        let id = id.strip_prefix("mem:").unwrap_or(id);
        let conn = self.conn.lock();
        let scope: Option<Option<String>> = conn.query_row(
            "SELECT c.team FROM mem_items m JOIN solution_metadata s ON s.item_id=m.id LEFT JOIN blueprint_call_scopes c ON c.item_id=m.id AND c.area=m.area WHERE m.id=?1 AND m.area=?2 AND m.kind='skill' AND m.source=?3",
            params![id, area, SOURCE], |row| row.get(0),
        ).optional().map_err(|e| e.to_string())?;
        let team = scope.ok_or("Saved Blueprint solution not found in this workspace")?;
        let (item, _) = store::managed_payload(&conn, id, &self.blobs_dir)?
            .ok_or("Saved Blueprint solution disappeared")?;
        let mut document = model::parse_document(&item.body)?;
        let dependencies = std::mem::take(&mut document.dependencies);
        if serde_json::to_vec(&document)
            .map_err(|e| e.to_string())?
            .len()
            > calls::MAX_CLOSURE_BYTES
        {
            return Err("Saved solution exceeds 256 KiB".into());
        }
        calls::projection::graph_resolved(&document, |pin| {
            if !calls::saved_reference(&pin.id) {
                let callee = dependencies.get(&pin.key()).ok_or("Missing local pin")?;
                if callee.pin() != *pin {
                    return Err("Local pin identity mismatch".into());
                }
                callee.verify()?;
                return Ok((*callee.document).clone());
            }
            call_revisions::ScopedResolver {
                conn: &conn,
                blobs: &self.blobs_dir,
                area,
                team: team.as_deref().ok_or("Saved call requires a Team")?,
            }
            .revision(pin)
        })
    }
}

#[cfg(test)]
mod tests;
