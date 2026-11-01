use super::{SqliteMemStore, WriteContext};
use crate::{backend::Deletion, store};

impl SqliteMemStore {
    pub(super) fn read_deletion_targets(
        &self,
        id: &str,
    ) -> Result<(Vec<String>, Option<String>), String> {
        let conn = self.conn.lock();
        let source = codeterm_mem_store::skill_folders::folder_skill_source_path(&conn, id)
            .map_err(|e| format!("read folder skill source: {e}"))?;
        let ids = match source.as_deref() {
            Some(source) => codeterm_mem_store::skill_folders::skill_ids_for_source(&conn, source)
                .map_err(|e| format!("list folder skill registrations: {e}"))?,
            None => vec![id.to_string()],
        };
        Ok((ids, source))
    }

    pub(super) fn read_area_ids(&self, area: &str) -> Vec<String> {
        let conn = self.conn.lock();
        let mut out = Vec::new();
        if let Ok(mut stmt) = conn.prepare("SELECT id FROM mem_items WHERE area = ?1") {
            if let Ok(rows) = stmt.query_map([area], |r| r.get::<_, String>(0)) {
                out.extend(rows.flatten());
            }
        }
        out
    }

    pub(super) fn remove_batch(
        &self,
        ctx: &WriteContext,
        ids: &[String],
    ) -> Result<Deletion, String> {
        let conn = self.conn.lock();
        let mut report = Deletion::default();
        for id in ids {
            let area = store::get(&conn, id, &self.blobs_dir)?
                .filter(|item| item.source.as_deref() == Some(crate::blueprint_library::SOURCE))
                .and_then(|item| item.area);
            let source = codeterm_mem_store::skill_folders::folder_skill_source_path(&conn, id)
                .map_err(|e| format!("read folder skill source: {e}"))?;
            if store::delete_with_context(ctx, &conn, id)? {
                report.ids.push(id.clone());
                report.library_areas.extend(area);
                report.skill_sources.push((id.clone(), source));
            }
        }
        Ok(report)
    }
}
