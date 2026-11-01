use super::*;
use crate::store;
impl SqliteMemStore {
    pub(super) fn pending_extraction_in(
        &self,
        limit: usize,
    ) -> Result<Vec<PendingExtraction>, String> {
        use crate::store::MemKind::*;
        let conn = self.conn.lock();
        // worker has already run against it — we don't need to re-queue.
        let mut stmt = conn
            .prepare(
                r#"SELECT m.id, m.kind, m.title, COALESCE(m.body_inline, '')
               FROM mem_items m
               WHERE m.kind IN ('conversation','note','daily','learning','fact')
                 AND m.tags_json NOT LIKE '%auto-extracted%'
                 AND NOT EXISTS (
                   SELECT 1 FROM relations r
                   WHERE r.from_id = m.id OR r.to_id = m.id
                 )
               ORDER BY m.created_at DESC
               LIMIT ?1"#,
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([limit as i64], |r| {
                let id: String = r.get(0)?;
                let kind_str: String = r.get(1)?;
                let title: String = r.get(2)?;
                let body: String = r.get(3)?;
                let kind = match kind_str.as_str() {
                    "conversation" => Conversation,
                    "note" => Note,
                    "daily" => Daily,
                    "learning" => Learning,
                    "fact" => Fact,
                    _ => Note,
                };
                Ok((id, title, body, kind))
            })
            .map_err(|e| e.to_string())?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    pub(super) fn pending_text_in(&self, id: &str) -> Option<String> {
        let conn = self.conn.lock();
        if let Ok(Some(item)) = store::get(&conn, id, &self.blobs_dir) {
            return crate::body_is_embeddable(&item.body).then(|| {
                crate::reindex::compose_item_embed_text(
                    &item.kind,
                    &item.title,
                    item.area.as_deref(),
                    &item.tags,
                    &item.body,
                    item.source.as_deref(),
                    item.summary.as_deref(),
                )
            });
        }
        conn.query_row(
            "SELECT entity_type,properties_json FROM entities WHERE id=?1",
            [id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .ok()
        .map(|(kind, props)| crate::embeddings::embeddable_text_for_entity(id, &kind, &props))
    }
}
