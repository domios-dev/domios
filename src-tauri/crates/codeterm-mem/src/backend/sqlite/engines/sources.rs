use super::*;
use rusqlite::params;
impl SqliteMemStore {
    pub(super) fn tracked_sources_in(&self) -> Result<Vec<TrackedSource>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id,source_path,source_size,source_mtime,tags_json FROM mem_items WHERE source_path IS NOT NULL").map_err(|e|e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok(TrackedSource {
                    id: r.get(0)?,
                    source_path: r.get(1)?,
                    source_size: r.get(2)?,
                    source_mtime: r.get(3)?,
                    tags_json: r.get(4)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }
    pub(super) fn refresh_source_in(
        &self,
        ctx: &WriteContext,
        expected: &TrackedSource,
        refresh: &SourceRefresh<'_>,
    ) -> Result<bool, String> {
        let conn = self.conn.lock();
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        let mut tags = serde_json::from_str::<Vec<String>>(&expected.tags_json).unwrap_or_default();
        tags.retain(|tag| tag != "source-missing");
        if refresh.missing {
            tags.push("source-missing".into());
        }
        let tags = serde_json::to_string(&tags).map_err(|e| e.to_string())?;
        let author = ctx.default_system_author("file-sync");
        let changed = tx.execute("UPDATE mem_items SET tags_json=?1,updated_at=?2,
            body_inline=CASE WHEN ?3 IS NULL THEN body_inline ELSE ?3 END,
            body_blob_path=CASE WHEN ?3 IS NULL THEN body_blob_path ELSE NULL END,
            source_size=CASE WHEN ?3 IS NULL THEN source_size ELSE ?4 END,
            source_mtime=CASE WHEN ?3 IS NULL THEN source_mtime ELSE ?5 END,
            author_uid=CASE WHEN ?3 IS NULL OR author_kind='neighbor' THEN author_uid ELSE ?6 END,
            author_friendship=CASE WHEN ?3 IS NULL OR author_kind='neighbor' THEN author_friendship ELSE ?7 END,
            author_label=CASE WHEN ?3 IS NULL OR author_kind='neighbor' THEN author_label ELSE ?8 END,
            author_kind=CASE WHEN ?3 IS NULL OR author_kind='neighbor' THEN author_kind ELSE ?9 END
            WHERE id=?10 AND source_path=?11 AND source_size IS ?12 AND source_mtime IS ?13 AND tags_json=?14",
            params![tags,chrono::Utc::now().to_rfc3339(),refresh.body,refresh.size,refresh.mtime,author.account_uid,author.friendship_id,author.label,author.kind.as_str(),expected.id,expected.source_path,expected.source_size,expected.source_mtime,expected.tags_json]).map_err(|e|e.to_string())?;
        if changed == 0 {
            return Ok(false);
        }
        if let Some(body) = refresh.body {
            tx.execute(
                "UPDATE mem_fts SET body=?1 WHERE mem_id=?2",
                params![body, expected.id],
            )
            .map_err(|e| e.to_string())?;
            index::clear_vectors(&tx, &expected.id);
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(true)
    }
}
