use super::*;
use codeterm_blueprint_types::blueprint_document::calls::{
    projection::ExpandedGraph, MemoryResolver,
};
use rusqlite::OptionalExtension;
impl SqliteMemStore {
    pub(super) fn solution_show(&self, area: &str, id: &str) -> Result<ExpandedGraph, String> {
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
        crate::blueprint_library::show::project(model::parse_document(&item.body)?, |pin| {
            calls::ScopedResolver {
                conn: &conn,
                blobs: &self.blobs_dir,
                area,
                team: team.as_deref().ok_or("Saved call requires a Team")?,
            }
            .revision(pin)
        })
    }
}
