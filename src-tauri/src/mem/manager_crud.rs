//! Mem CRUD operations: save, get, update, delete.

#[cfg(feature = "embeddings")]
use super::reindex::compose_embed_text;
use super::store::MemItem;
use super::{store, MemManager, NewMemItem};

impl MemManager {
    pub fn save(&self, item: NewMemItem) -> Result<String, String> {
        self.save_inner(item, false)
    }

    /// Raw insert that skips dedup, auto-extraction of child items, and
    /// the embed queue. Intended for bench/eval seeding where we want to
    /// control exactly which items end up in the store. Returns the
    /// generated id (same format as [`save`]).
    pub fn insert_raw(&self, item: &NewMemItem) -> Result<String, String> {
        let conn = self.conn.lock();
        store::insert(&conn, item, &self.blobs_dir)
    }

    pub fn save_replace(&self, item: NewMemItem) -> Result<String, String> {
        self.save_inner(item, true)
    }

    fn save_inner(&self, item: NewMemItem, replace: bool) -> Result<String, String> {
        let conn = self.conn.lock();

        if !replace && !item.title.is_empty() {
            let existing: Option<String> = conn
                .query_row(
                    "SELECT id FROM mem_items WHERE kind = ?1 AND title = ?2 LIMIT 1",
                    rusqlite::params![item.kind.as_str(), &item.title],
                    |r| r.get(0),
                )
                .ok();
            if let Some(dup_id) = existing {
                log::info!(
                    "[mem] dedup: bumping recurrence on existing {} (same kind+title)",
                    dup_id
                );
                let _ = conn.execute(
                    "UPDATE mem_items SET recurrence_count = recurrence_count + 1,
                                         accessed_at = ?1
                     WHERE id = ?2",
                    rusqlite::params![chrono::Utc::now().to_rfc3339(), &dup_id],
                );
                return Ok(dup_id);
            }
        }

        let id = if replace {
            store::upsert(&conn, &item, &self.blobs_dir)?
        } else {
            store::insert(&conn, &item, &self.blobs_dir)?
        };

        #[cfg(feature = "embeddings")]
        {
            let body_text = if let Some(ref src) = item.source {
                if let Some(path) = src.strip_prefix("file:") {
                    std::fs::read_to_string(path).unwrap_or_else(|_| item.body.clone())
                } else {
                    item.body.clone()
                }
            } else {
                item.body.clone()
            };
            // Only embed when the body carries real content. Empty/thin items would
            // otherwise index a title-only vector — pure noise in vector search.
            if super::body_is_embeddable(&body_text) {
                let embed_text = compose_embed_text(
                    &format!("{:?}", item.kind).to_lowercase(),
                    &item.title,
                    item.area.as_deref(),
                    &item.tags,
                    &body_text,
                );
                self.embed_queue.enqueue(id.clone(), embed_text);
            }
        }
        Ok(id)
    }

    pub fn get(&self, id: &str) -> Result<Option<MemItem>, String> {
        let conn = self.conn.lock();
        store::get(&conn, id, &self.blobs_dir)
    }

    pub fn delete(&self, id: &str) -> Result<bool, String> {
        let conn = self.conn.lock();
        store::delete(&conn, id)
    }

    pub fn delete_by_area(&self, area: &str) -> Result<usize, String> {
        let conn = self.conn.lock();
        let ids: Vec<String> = {
            let mut out = Vec::new();
            if let Ok(mut stmt) = conn.prepare("SELECT id FROM mem_items WHERE area = ?1") {
                if let Ok(rows) = stmt.query_map(rusqlite::params![area], |r| r.get::<_, String>(0))
                {
                    for row in rows.flatten() {
                        out.push(row);
                    }
                }
            }
            out
        };
        let mut count = 0;
        for id in &ids {
            if store::delete(&conn, id)? {
                count += 1;
            }
        }
        Ok(count)
    }

    pub fn update(&self, id: &str, patch: store::MemUpdate) -> Result<bool, String> {
        let body_changed = patch.body.is_some() || patch.title.is_some();
        let updated = {
            let conn = self.conn.lock();
            store::update(&conn, id, &patch, &self.blobs_dir)?
        };
        if updated && body_changed {
            #[cfg(feature = "embeddings")]
            {
                let conn = self.conn.lock();
                if let Ok(Some(item)) = store::get(&conn, id, &self.blobs_dir) {
                    drop(conn);
                    // Re-embed only meaningful bodies; vectors left stale by a body
                    // that shrank below the bar are swept by the dreamer's cleanup.
                    if super::body_is_embeddable(&item.body) {
                        let embed_text = compose_embed_text(
                            &format!("{:?}", item.kind).to_lowercase(),
                            &item.title,
                            item.area.as_deref(),
                            &item.tags,
                            &item.body,
                        );
                        self.embed_queue.enqueue(id.to_string(), embed_text);
                    }
                }
            }
        }
        Ok(updated)
    }
}
