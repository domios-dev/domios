//! Mem CRUD operations: save, get, update, delete.

#[cfg(feature = "embeddings")]
use super::reindex::compose_embed_text;
use super::store::MemItem;
use super::{store, MemManager, NewMemItem};

/// What a batch forget actually did — deleted rows and the extraction
/// generations that lost authority with them, reported separately because they
/// are different numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AreaForgetOutcome {
    pub deleted: usize,
    pub cancelled_extractions: usize,
}

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

    /// Create a bundled item with its fingerprint in the same insert —
    /// never a two-step create-then-bootstrap. Keeps `save`'s embed queue.
    pub fn save_seeded(&self, item: NewMemItem, fingerprint: &str) -> Result<String, String> {
        self.guard_commit()?;
        let conn = self.conn.lock();
        let id = store::insert_seeded(&conn, &item, &self.blobs_dir, fingerprint)?;
        drop(conn);

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

    fn save_inner(&self, item: NewMemItem, replace: bool) -> Result<String, String> {
        self.guard_commit()?;
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
        drop(conn);

        // Replace destroys the row the in-flight generation copied its text from,
        // so that generation is superseded exactly like a delete. Revoked outside
        // the connection lock — the cancel sink closes tabs.
        if replace {
            self.revoke_extraction(&id);
        }

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

    /// Revoke the deleted source's extraction generations and log what was
    /// cancelled. Returns the revocations so callers can report the cleanup.
    pub fn revoke_extraction(&self, id: &str) -> Vec<super::RevokedGeneration> {
        let revoked = self.extraction.revoke_source(id);
        for r in &revoked {
            log::info!(
                "[mem] revoked extraction generation {} (worker {})",
                r.generation.token(),
                r.worker_id.as_deref().unwrap_or("not yet spawned"),
            );
        }
        revoked
    }

    pub fn delete(&self, id: &str) -> Result<bool, String> {
        let removed = {
            let conn = self.conn.lock();
            store::delete(&conn, id)?
        };
        if removed {
            self.revoke_extraction(id);
        }
        Ok(removed)
    }

    pub fn delete_by_area(&self, area: &str) -> Result<AreaForgetOutcome, String> {
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
        let mut deleted = Vec::new();
        for id in &ids {
            if store::delete(&conn, id)? {
                deleted.push(id.clone());
            }
        }
        drop(conn);

        // Batch forget shares the single-delete lifecycle: each deleted source
        // revokes its own generations, so unrelated extractions are untouched.
        let cancelled = deleted
            .iter()
            .flat_map(|id| self.revoke_extraction(id))
            .count();
        Ok(AreaForgetOutcome {
            deleted: deleted.len(),
            cancelled_extractions: cancelled,
        })
    }

    /// Every field the bundled seeder manages for `id`, plus its recorded
    /// `seed_fingerprint`. Never bumps access stats — see `store::managed_payload`.
    pub fn managed_payload(&self, id: &str) -> Result<Option<(NewMemItem, Option<String>)>, String> {
        let conn = self.conn.lock();
        store::managed_payload(&conn, id, &self.blobs_dir)
    }

    /// Overwrite `id`'s bundled content only when its fingerprint still
    /// matches `expected_fingerprint`; see `store::cas_refresh_bundled`.
    pub fn cas_refresh_bundled(
        &self,
        id: &str,
        expected_fingerprint: Option<&str>,
        new_item: &NewMemItem,
        new_fingerprint: &str,
    ) -> Result<bool, String> {
        let conn = self.conn.lock();
        store::cas_refresh_bundled(
            &conn,
            id,
            &self.blobs_dir,
            expected_fingerprint,
            new_item,
            new_fingerprint,
        )
    }

    /// Record a fingerprint for a pre-fingerprint item without touching any
    /// managed field; see `store::bootstrap_seed_fingerprint`.
    pub fn bootstrap_seed_fingerprint(
        &self,
        id: &str,
        expected: &NewMemItem,
        fingerprint: &str,
    ) -> Result<bool, String> {
        let conn = self.conn.lock();
        store::bootstrap_seed_fingerprint(&conn, id, &self.blobs_dir, expected, fingerprint)
    }

    pub fn update(&self, id: &str, patch: store::MemUpdate) -> Result<bool, String> {
        self.guard_commit()?;
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
