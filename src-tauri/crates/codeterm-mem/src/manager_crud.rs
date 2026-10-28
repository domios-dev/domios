//! Mem CRUD operations: save, get, update, delete.

use super::reindex::compose_item_embed_text;
use super::store::MemItem;
use super::{store, MemManager, NewMemItem};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveOutcome {
    pub id: String,
    pub outcome: String,
}

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
        self.save_inner(item, false).map(|outcome| outcome.id)
    }

    /// Raw insert that skips dedup, auto-extraction of child items, and
    /// the embed queue. Intended for bench/eval seeding where we want to
    /// control exactly which items end up in the store. Returns the
    /// generated id (same format as [`save`]).
    pub fn insert_raw(&self, item: &NewMemItem) -> Result<String, String> {
        self.store.insert(&Self::write_context(), item)
    }

    pub fn save_replace(&self, item: NewMemItem) -> Result<String, String> {
        self.save_inner(item, true).map(|outcome| outcome.id)
    }

    pub fn save_with_outcome(
        &self,
        item: NewMemItem,
        replace: bool,
    ) -> Result<SaveOutcome, String> {
        self.save_inner(item, replace)
    }

    /// Create a bundled item with its fingerprint in the same insert —
    /// never a two-step create-then-bootstrap. Keeps `save`'s embed queue.
    pub fn save_seeded(&self, item: NewMemItem, fingerprint: &str) -> Result<String, String> {
        self.guard_commit()?;
        let id = self
            .store
            .insert_seeded(&Self::write_context(), &item, fingerprint)?;

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
                let embed_text = compose_item_embed_text(
                    &format!("{:?}", item.kind).to_lowercase(),
                    &item.title,
                    item.area.as_deref(),
                    &item.tags,
                    &body_text,
                    item.source.as_deref(),
                    item.summary.as_deref(),
                );
                self.embed_queue.enqueue(id.clone(), embed_text);
            }
        }
        Ok(id)
    }

    fn save_inner(&self, item: NewMemItem, replace: bool) -> Result<SaveOutcome, String> {
        self.save_with_context(item, replace, Self::write_context())
            .map_err(|e| e.to_string())
    }

    pub fn save_with_context(
        &self,
        item: NewMemItem,
        replace: bool,
        ctx: crate::backend::WriteContext,
    ) -> Result<SaveOutcome, crate::backend::SaveError> {
        self.guard_commit()?;
        let outcome = self.store.save(&ctx, &item, replace)?;
        if outcome.outcome == "unchanged" {
            return Ok(outcome);
        }
        if outcome.outcome == "updated" {
            self.revoke_extraction(&outcome.id);
            self.enqueue_embedding(&outcome.id, &item);
            return Ok(outcome);
        }
        if replace {
            self.revoke_extraction(&outcome.id);
            self.embed_queue.cancel(&outcome.id);
        }
        let body_text = item
            .source
            .as_deref()
            .and_then(|src| src.strip_prefix("file:"))
            .and_then(|path| std::fs::read_to_string(path).ok())
            .unwrap_or_else(|| item.body.clone());
        if super::body_is_embeddable(&body_text) {
            let text = compose_item_embed_text(
                &format!("{:?}", item.kind).to_lowercase(),
                &item.title,
                item.area.as_deref(),
                &item.tags,
                &body_text,
                item.source.as_deref(),
                item.summary.as_deref(),
            );
            self.embed_queue.enqueue(outcome.id.clone(), text);
        }
        Ok(outcome)
    }

    fn enqueue_embedding(&self, id: &str, item: &NewMemItem) {
        self.embed_queue.cancel(id);
        if super::body_is_embeddable(&item.body) {
            let embed_text = compose_item_embed_text(
                &format!("{:?}", item.kind).to_lowercase(),
                &item.title,
                item.area.as_deref(),
                &item.tags,
                &item.body,
                item.source.as_deref(),
                item.summary.as_deref(),
            );
            self.embed_queue.enqueue(id.to_string(), embed_text);
        }
    }

    pub fn get(&self, id: &str) -> Result<Option<MemItem>, String> {
        self.store.get(id)
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
        let (ids, source) = self.store.deletion_targets(id)?;
        for id in &ids {
            self.embed_queue.cancel(id);
        }
        let removed = self.store.delete_batch(&Self::write_context(), &ids)?;
        self.notify_blueprint_library_changed(removed.library_areas);
        if removed.ids.iter().any(|removed| removed == id) {
            remove_folder_skill(source.as_deref(), &self.skill_dir, id);
        }
        for id in &removed.ids {
            self.revoke_extraction(id);
        }
        Ok(removed.ids.iter().any(|removed| removed == id))
    }

    pub fn delete_by_area(&self, area: &str) -> Result<AreaForgetOutcome, String> {
        let ids = self.store.ids_by_area(area);
        for id in &ids {
            self.embed_queue.cancel(id);
        }
        let removed = self.store.delete_batch(&Self::write_context(), &ids)?;
        if !removed.library_areas.is_empty() {
            self.notify_blueprint_library_changed([area.to_owned()]);
        }
        for (id, source) in removed.skill_sources {
            remove_folder_skill(source.as_deref(), &self.skill_dir, &id);
        }
        let cancelled = removed
            .ids
            .iter()
            .map(|id| self.revoke_extraction(id).len())
            .sum();
        Ok(AreaForgetOutcome {
            deleted: removed.ids.len(),
            cancelled_extractions: cancelled,
        })
    }

    /// Every field the bundled seeder manages for `id`, plus its recorded
    /// `seed_fingerprint`. Never bumps access stats — see `store::managed_payload`.
    pub fn managed_payload(
        &self,
        id: &str,
    ) -> Result<Option<(NewMemItem, Option<String>)>, String> {
        self.store.managed_payload(id)
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
        self.store.cas_refresh_bundled(
            &Self::write_context(),
            id,
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
        self.store
            .bootstrap_seed_fingerprint(&Self::write_context(), id, expected, fingerprint)
    }

    pub fn update(&self, id: &str, patch: store::MemUpdate) -> Result<bool, String> {
        self.guard_commit()?;
        let body_changed = patch.body.is_some() || patch.title.is_some();
        let updated = { self.store.update(&Self::write_context(), id, &patch)? };
        if updated && body_changed {
            self.refresh_embedding(id);
        }
        Ok(updated)
    }

    /// Re-queue an item's embedding after a write that went through the store
    /// directly, such as a merge or restore committed in one transaction.
    pub fn refresh_embedding(&self, id: &str) {
        self.embed_queue.cancel(id);
        {
            if let Ok(Some(item)) = self.store.get(id) {
                // that shrank below the bar are swept by the dreamer's cleanup.
                if super::body_is_embeddable(&item.body) {
                    let embed_text = compose_item_embed_text(
                        &format!("{:?}", item.kind).to_lowercase(),
                        &item.title,
                        item.area.as_deref(),
                        &item.tags,
                        &item.body,
                        item.source.as_deref(),
                        item.summary.as_deref(),
                    );
                    self.embed_queue.enqueue(id.to_string(), embed_text);
                }
            }
        }
    }
}

fn remove_folder_skill(source: Option<&str>, skills_root: &std::path::Path, id: &str) {
    let Some(folder) = source.and_then(|source| {
        codeterm_mem_store::skill_folders::folder_skill_folder_to_remove(source, skills_root)
    }) else {
        return;
    };
    if let Err(error) = std::fs::remove_dir_all(&folder) {
        log::warn!(
            "[mem] could not remove skill folder for {} at {}: {}",
            id,
            folder.display(),
            error
        );
    }
}

#[cfg(test)]
mod tests;
