use super::*;
use crate::{embeddings, store};

impl SqliteMemStore {
    pub(super) fn item_targets_in(
        &self,
        conn: &rusqlite::Connection,
    ) -> Result<Vec<(String, String)>, String> {
        let _read = UntouchedRead::begin(conn)?;
        let mut statement = conn
            .prepare("SELECT id FROM mem_items WHERE kind != 'archived'")
            .map_err(|error| error.to_string())?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        let mut targets = Vec::new();
        for id in ids {
            let Some(item) = store::get(conn, &id, &self.blobs_dir)? else {
                continue;
            };
            if !crate::body_is_embeddable(&item.body) {
                continue;
            }
            let text = crate::reindex::compose_item_embed_text(
                &item.kind,
                &item.title,
                item.area.as_deref(),
                &item.tags,
                &item.body,
                item.source.as_deref(),
                item.summary.as_deref(),
            );
            targets.push((item.id, text));
        }
        Ok(targets)
    }

    pub(super) fn entity_targets_in(
        &self,
        conn: &rusqlite::Connection,
    ) -> Result<Vec<(String, String)>, String> {
        let mut stmt = conn
            .prepare("SELECT id, entity_type, properties_json FROM entities")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        let mut targets = Vec::new();
        for row in rows {
            let (id, entity_type, props_json) = row.map_err(|e| e.to_string())?;
            let text = embeddings::embeddable_text_for_entity(&id, &entity_type, &props_json);
            targets.push((id, text));
        }
        Ok(targets)
    }
}
/// Hydration touches every read item; this savepoint is always rolled back, even on unwind.
/// It also discards hydration's opportunistic file-source refresh, which the next read redoes.
pub(crate) struct UntouchedRead<'c>(&'c rusqlite::Connection);

impl<'c> UntouchedRead<'c> {
    const BEGIN: &'static str = "SAVEPOINT mem_untouched_read";

    pub(crate) fn begin(conn: &'c rusqlite::Connection) -> Result<Self, String> {
        conn.execute_batch(Self::BEGIN)
            .map_err(|e| format!("begin untouched read: {e}"))?;
        Ok(Self(conn))
    }
}

impl Drop for UntouchedRead<'_> {
    fn drop(&mut self) {
        for statement in [
            "ROLLBACK TO mem_untouched_read",
            "RELEASE mem_untouched_read",
        ] {
            if let Err(error) = self.0.execute_batch(statement) {
                log::warn!("[mem.search] untouched read cleanup failed: {error}");
            }
        }
    }
}
