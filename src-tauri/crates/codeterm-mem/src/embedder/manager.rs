use std::sync::Arc;

use super::active::{ActiveEmbedder, EmbedderEvent};
use super::provider::SharedProvider;
use super::switch::SwitchPlan;
use super::{index, pending, EmbedderStatus};
use crate::{embeddings, store, MemManager};

impl MemManager {
    pub fn embedder(&self) -> &Arc<ActiveEmbedder> {
        &self.embedder
    }

    pub fn embedder_status(&self) -> EmbedderStatus {
        let provider = self.embedder.provider();
        let identity = provider.identity();
        let (index, vectors, pending) = {
            let conn = self.conn.lock();
            (
                index::stamp(&conn).ok().flatten(),
                index::vector_count(&conn),
                pending::count(&conn),
            )
        };
        let reembed = self.reembed.snapshot();
        let degraded = !self.embedder.is_available()
            || !matches!(
                reembed.state,
                super::ReembedState::Completed | super::ReembedState::Idle
            )
            || index.as_ref() != Some(&identity);
        EmbedderStatus {
            provider: identity.provider.clone(),
            model: identity.model.clone(),
            dimension: identity.dimension,
            remote: provider.remote(),
            available: self.embedder.is_available(),
            index_matches: index.as_ref() == Some(&identity),
            index,
            vectors,
            pending,
            degraded,
            reembed,
        }
    }

    /// How much a switch would re-embed: every embeddable item, every entity
    /// and, for the daemon's manager, every documentation section.
    pub fn reembed_plan(&self) -> Result<SwitchPlan, String> {
        let blueprints =
            crate::blueprint_library::index::indexed_items(&self.conn.lock())?.len() as u64;
        Ok(SwitchPlan {
            blueprints,
            items: self.reembed_item_targets()?.len() as u64,
            entities: self.reembed_entity_targets()?.len() as u64,
            docs: if self.docs_dir.is_some() {
                crate::docs_source::vector_section_count() as u64
            } else {
                0
            },
        })
    }

    /// Build a stamped shadow while recall continues on the published model.
    pub fn switch_embedder(
        self: &Arc<Self>,
        provider: SharedProvider,
    ) -> Result<SwitchPlan, String> {
        let identity = provider.identity();
        if self.embedder.identity() == identity
            && index::stamp(&self.conn.lock())?.as_ref() == Some(&identity)
        {
            return Ok(SwitchPlan::default());
        }
        if self
            .reembed_busy
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            return Err(
                "a re-embed worker is still in flight; inspect embedder status before retrying"
                    .into(),
            );
        }
        let result = (|| {
            let plan = self.reembed_plan()?;
            self.prepare_shadow(&identity)?;
            let generation = self.reembed.start(identity, plan, &self.embedder);
            if let Err(error) = self.launch_shadow(generation, ActiveEmbedder::new(provider)) {
                self.reembed.fail(generation, error.clone(), &self.embedder);
                return Err(error);
            }
            Ok(plan)
        })();
        if result.is_err() {
            self.reembed_busy
                .store(false, std::sync::atomic::Ordering::SeqCst);
        }
        result
    }

    /// Adopt `provider` at startup: switch when the index is stamped for a
    /// different model, otherwise just make it active.
    pub fn reconcile_embedder(
        self: &Arc<Self>,
        provider: SharedProvider,
    ) -> Result<Option<SwitchPlan>, String> {
        let identity = provider.identity();
        let stamp = index::stamp(&self.conn.lock())?;
        if stamp.as_ref() == Some(&identity) {
            if self.embedder.identity() != identity {
                self.embedder.replace(provider);
                self.rebuild_docs_vectors();
            }
            self.backfill_pending();
            return Ok(None);
        }
        self.switch_embedder(provider).map(Some)
    }

    /// Queue every vector-pending item and entity again.
    pub fn backfill_pending(&self) -> usize {
        let ids = match pending::ids(&self.conn.lock()) {
            Ok(ids) => ids,
            Err(error) => {
                log::warn!("[mem.embedder] read vector pending: {error}");
                return 0;
            }
        };
        let mut queued = 0;
        for id in ids {
            match self.embed_text_for(&id) {
                Some(text) => {
                    self.embed_queue.enqueue(id, text);
                    queued += 1;
                }
                None => {
                    let _ = pending::clear(&self.conn.lock(), &id);
                }
            }
        }
        if queued > 0 {
            log::info!("[mem.embedder] backfilling {queued} vector-pending item(s)");
        }
        queued
    }

    fn rebuild_docs_vectors(&self) {
        let Some(data_dir) = self.docs_dir.clone() else {
            return;
        };
        let embedder = self.embedder.clone();
        let spawned = std::thread::Builder::new()
            .name("docs-vector-build".into())
            .spawn(move || {
                if let Err(error) = crate::docs_source::reembed_vectors(&data_dir, &embedder) {
                    log::warn!("[docs.vectors] rebuild failed: {error}");
                }
            });
        if let Err(error) = spawned {
            log::warn!("[docs.vectors] rebuild not started: {error}");
        }
    }

    pub(crate) fn subscribe_recovery(self: &Arc<Self>) {
        let manager = Arc::downgrade(self);
        self.embedder.subscribe(Arc::new(move |event| {
            if matches!(event, EmbedderEvent::Recovered) {
                if let Some(manager) = manager.upgrade() {
                    manager.backfill_pending();
                    if let Err(error) = manager.queue_blueprint_backfill() {
                        log::warn!("[blueprint-memory] recovery backfill not queued: {error}");
                    }
                }
            }
        }));
    }

    fn embed_text_for(&self, id: &str) -> Option<String> {
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
            "SELECT entity_type, properties_json FROM entities WHERE id = ?1",
            [id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .ok()
        .map(|(entity_type, props)| {
            embeddings::embeddable_text_for_entity(id, &entity_type, &props)
        })
    }
}
