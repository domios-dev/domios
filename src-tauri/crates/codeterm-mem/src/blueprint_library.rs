use crate::MemManager;
use codeterm_blueprint_types::{
    blueprint_document::Document,
    blueprint_memory::{self as model, Entry, Evidence, AREA_CAP},
};
use std::sync::atomic::{AtomicU64, Ordering};
pub mod authoring;
pub mod call_revisions;
pub mod catalog;
mod changes;
pub mod dedupe;
pub mod enrichment;
#[cfg(test)]
pub(crate) mod fakes;
pub mod index;
pub mod merge_policy;
pub mod qualification;
pub mod recall;
pub mod services;
pub(crate) mod show;
use qualification::PreparedBlueprint;
pub use recall::{BlueprintHit, Fusion, RecallConfig, Weights};
pub use services::{BlueprintEnricher, EnrichmentJob};

pub const SOURCE: &str = "blueprint-library";
static RECALLS: AtomicU64 = AtomicU64::new(0);
static HITS: AtomicU64 = AtomicU64::new(0);
static DREAM_AREA: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct SaveResult {
    pub id: String,
    pub evicted_ids: Vec<String>,
}

pub(crate) enum Pending {
    Keep {
        area: String,
        prepared: PreparedBlueprint,
        reply: std::sync::mpsc::SyncSender<Result<SaveResult, String>>,
    },
    Reuse(Vec<String>),
    Backfill,
}

impl MemManager {
    pub fn blueprint_tags(&self, id: &str) -> Result<Vec<String>, String> {
        if id.starts_with("catalog:") {
            return Ok(catalog::tags(id).iter().map(|tag| (*tag).into()).collect());
        }
        let id = id
            .strip_prefix("mem:")
            .or_else(|| id.strip_prefix("memory:"))
            .unwrap_or(id);
        Ok(self
            .get(id)?
            .map(|item| {
                item.tags
                    .into_iter()
                    .filter(|tag| codeterm_blueprint_types::blueprint_tags::contains(tag))
                    .collect()
            })
            .unwrap_or_default())
    }

    fn queue_blueprint(self: &std::sync::Arc<Self>, pending: Pending) -> Result<(), String> {
        let mut queue = self.blueprint.queue.lock();
        if queue.is_none() {
            let (sender, receiver) = std::sync::mpsc::sync_channel::<Pending>(16);
            let weak = std::sync::Arc::downgrade(self);
            std::thread::Builder::new()
                .name("blueprint-memory".into())
                .spawn(move || {
                    while let Ok(pending) = receiver.recv() {
                        let Some(mem) = weak.upgrade() else { break };
                        let result = match pending {
                            Pending::Keep {
                                area,
                                prepared,
                                reply,
                            } => {
                                let saved = mem.persist_blueprint(&area, prepared, true, AREA_CAP);
                                let id = saved.as_ref().ok().map(|saved| saved.id.clone());
                                let _ = reply.send(saved);
                                id.map_or(Ok(()), |id| mem.index_blueprint(&id).map(|_| ()))
                            }
                            Pending::Reuse(ids) => mem.reuse_blueprints(&ids),
                            Pending::Backfill => mem.backfill_blueprints().map(|_| ()),
                        };
                        if let Err(error) = result {
                            log::error!("[blueprint-memory] persistence failed: {error}");
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
            *queue = Some(sender);
        }
        queue
            .as_ref()
            .ok_or("Blueprint memory queue unavailable")?
            .try_send(pending)
            .map_err(|_| "Blueprint memory queue full or closed".into())
    }

    pub fn enqueue_blueprint(
        self: &std::sync::Arc<Self>,
        area: String,
        definition: serde_json::Value,
        intents: Vec<String>,
        evidence: Evidence,
    ) -> Result<SaveResult, String> {
        self.guard_commit()?;
        let prepared = self.qualify_blueprint(
            &area,
            Document::from_definition(&definition)?,
            &intents,
            evidence,
        )?;
        let (reply, completion) = std::sync::mpsc::sync_channel(1);
        self.queue_blueprint(Pending::Keep {
            area,
            prepared,
            reply,
        })?;
        completion
            .recv()
            .map_err(|_| "Blueprint memory worker stopped before save completion")?
    }

    pub fn save_blueprint(
        &self,
        area: &str,
        document: Document,
        intents: &[String],
        evidence: Evidence,
    ) -> Result<SaveResult, String> {
        let prepared = self.qualify_blueprint(area, document, intents, evidence)?;
        let saved = self.persist_blueprint(area, prepared, true, AREA_CAP)?;
        self.index_after_save(&saved.id);
        Ok(saved)
    }

    pub fn save_blueprint_without_merge(
        &self,
        area: &str,
        document: Document,
        intents: &[String],
        evidence: Evidence,
    ) -> Result<SaveResult, String> {
        let prepared = self.qualify_blueprint(area, document, intents, evidence)?;
        self.persist_blueprint(area, prepared, false, AREA_CAP)
    }

    fn index_after_save(&self, id: &str) {
        if let Err(error) = self.index_blueprint(id) {
            log::warn!("[blueprint-memory] index {id}: {error}");
        }
    }

    fn persist_blueprint(
        &self,
        area: &str,
        prepared: PreparedBlueprint,
        merge: bool,
        area_cap: usize,
    ) -> Result<SaveResult, String> {
        self.guard_commit()?;
        let purpose = merge_policy::callable(&prepared.document)
            .then(|| merge_policy::Purpose::candidate(self, &prepared.intents));
        let saved = self.store.blueprint_save(
            &Self::blueprint_write_context(),
            area,
            prepared,
            merge,
            area_cap,
            purpose,
            &self.embedder,
        )?;
        self.notify_blueprint_library_changed([area.to_owned()]);
        self.refresh_embedding(&saved.id);
        Ok(saved)
    }

    fn reuse_blueprints(&self, ids: &[String]) -> Result<(), String> {
        self.store
            .blueprint_reuse(&Self::blueprint_write_context(), ids)
    }

    fn blueprint_write_context() -> crate::backend::WriteContext {
        let ctx = Self::write_context();
        let author = ctx.default_system_author("blueprint");
        ctx.with_author(author)
    }

    pub fn recall_blueprints(
        self: &std::sync::Arc<Self>,
        area: &str,
        intent: &str,
        limit: usize,
    ) -> Result<Vec<BlueprintHit>, String> {
        let result = if intent.trim().is_empty() {
            let now = chrono::Utc::now().timestamp();
            let mut library = self.list_blueprint_solutions(area)?;
            library.sort_by(|a, b| {
                model::score(b, now)
                    .total_cmp(&model::score(a, now))
                    .then(a.id.cmp(&b.id))
            });
            library.truncate(limit);
            library
                .into_iter()
                .map(|entry| BlueprintHit {
                    score: model::score(&entry, now),
                    why: "most used".into(),
                    enrichment_pending: false,
                    entry,
                })
                .collect()
        } else {
            self.recall_blueprint_hits(area, intent, None, limit, &RecallConfig::default())?
        };
        let total = RECALLS.fetch_add(1, Ordering::Relaxed) + 1;
        if !result.is_empty() {
            HITS.fetch_add(1, Ordering::Relaxed);
        }
        log::debug!(
            "[blueprint-memory] recall_hits={} recalls={total} hit_rate={:.3}",
            HITS.load(Ordering::Relaxed),
            HITS.load(Ordering::Relaxed) as f64 / total as f64
        );
        if !result.is_empty() && !intent.trim().is_empty() {
            let _ = self.queue_blueprint(Pending::Reuse(
                result.iter().map(|hit| hit.entry.id.clone()).collect(),
            ));
        }
        Ok(result)
    }

    pub fn list_blueprint_solutions(&self, area: &str) -> Result<Vec<Entry>, String> {
        self.store
            .blueprint_entries(&Self::blueprint_write_context(), area, true, None)
    }

    pub fn blueprint_solutions_readonly(&self, area: &str) -> Result<Vec<Entry>, String> {
        self.store.blueprint_readonly(area)
    }

    pub fn archive_blueprint_solution(&self, area: &str, id: &str) -> Result<(), String> {
        self.guard_commit()?;
        self.store
            .blueprint_archive(&Self::blueprint_write_context(), area, id)?;
        self.notify_blueprint_library_changed([area.to_owned()]);
        Ok(())
    }

    pub fn blueprint_recipe(
        self: &std::sync::Arc<Self>,
        area: &str,
        id: &str,
    ) -> Result<Document, String> {
        let library = self.list_blueprint_solutions(area)?;
        let entry = library
            .into_iter()
            .find(|entry| entry.id == id)
            .ok_or("Saved Blueprint recipe not found in this project")?;
        Ok(entry.document)
    }

    pub fn record_blueprint_reuse(self: &std::sync::Arc<Self>, id: &str) {
        let _ = self.queue_blueprint(Pending::Reuse(vec![id.into()]));
    }

    pub fn dream_blueprints(&self) -> Result<usize, String> {
        let changed = self.store.blueprint_dream(
            &Self::blueprint_write_context(),
            &self.embedder,
            DREAM_AREA.fetch_add(1, Ordering::Relaxed),
        )?;
        self.notify_blueprint_library_changed(changed.areas);
        for id in &changed.ids {
            self.refresh_embedding(id);
        }
        Ok(changed.ids.len())
    }
}

#[cfg(test)]
mod tests;
