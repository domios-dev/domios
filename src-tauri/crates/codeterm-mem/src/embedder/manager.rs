use std::sync::Arc;

use super::active::{ActiveEmbedder, EmbedderEvent};
use super::provider::SharedProvider;
use super::switch::SwitchPlan;
use super::EmbedderStatus;
use crate::MemManager;

impl MemManager {
    pub fn embedder(&self) -> &Arc<ActiveEmbedder> {
        &self.embedder
    }

    pub fn reembed_progress(&self) -> super::ReembedProgress {
        self.reembed.snapshot()
    }

    pub fn embedder_status(&self) -> EmbedderStatus {
        let provider = self.embedder.provider();
        let identity = provider.identity();
        let (index, vectors, pending) = self.store.engine_status(true).unwrap_or_default();
        let reembed = self.reembed.snapshot();
        // A build or a failed build leaves recall fully semantic on the published model.
        let degraded = !self.embedder.is_available() || index.as_ref() != Some(&identity);
        let mut queue = self.embed_queue.status(!self.embedder.is_available());
        crate::docs_source::work::add_status(&mut queue);
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
            queue,
            degraded,
            reembed,
        }
    }

    /// How much a switch would re-embed: every embeddable item, every entity
    /// and, for the daemon's manager, every documentation section.
    pub fn reembed_plan(&self) -> Result<SwitchPlan, String> {
        self.store.engine_plan(self.docs_count())
    }

    fn docs_count(&self) -> u64 {
        if self.docs_dir.is_some() {
            self.docs_sections().len() as u64
        } else {
            0
        }
    }

    /// Build a stamped shadow while recall continues on the published model.
    /// Choosing the published model cancels a running build; another target
    /// is refused until the running one ends.
    pub fn switch_embedder(
        self: &Arc<Self>,
        provider: SharedProvider,
    ) -> Result<SwitchPlan, String> {
        self.switch_embedder_with(provider, true)
    }

    pub fn try_switch_embedder(
        self: &Arc<Self>,
        provider: SharedProvider,
    ) -> Result<SwitchPlan, String> {
        self.switch_embedder_with(provider, false)
    }

    fn switch_embedder_with(
        self: &Arc<Self>,
        provider: SharedProvider,
        wait: bool,
    ) -> Result<SwitchPlan, String> {
        let identity = provider.identity();
        let running = self.reembed.snapshot();
        let failed = running.state == super::ReembedState::Failed;
        let running = (running.state == super::ReembedState::Running).then_some(running);
        if self.embedder.identity() == identity
            && self.store.engine_status(wait)?.0.as_ref() == Some(&identity)
        {
            if let Some(running) = running {
                let reason = format!(
                    "cancelled: {}/{} stays published",
                    identity.provider, identity.model
                );
                if !self.reembed.cancel(reason, &self.embedder) {
                    return Err(format!(
                        "the re-embed to {} is already publishing",
                        running
                            .target
                            .map_or_else(String::new, |t| format!("{}/{}", t.provider, t.model))
                    ));
                }
            }
            if failed {
                self.reembed.cancel(
                    "cancelled: failed shadow discarded; published model retained".into(),
                    &self.embedder,
                );
            }
            self.store.clear_shadow(&Self::write_context(), wait)?;
            super::local::retain_models(&identity, None);
            return Ok(SwitchPlan::default());
        }
        if let Some(running) = running {
            if running.target.as_ref() == Some(&identity) {
                return Ok(running.plan);
            }
            return Err(
                "another re-embed is running; switch back to the published model to cancel it first"
                    .into(),
            );
        }
        let plan = self.store.prepare_engine_switch(
            &Self::write_context(),
            &identity,
            self.docs_count(),
            wait,
        )?;
        let generation = self.reembed.start(identity, plan, &self.embedder);
        if let Err(error) = self.launch_shadow(generation, ActiveEmbedder::new(provider)) {
            self.reembed.fail(generation, error.clone(), &self.embedder);
            return Err(error);
        }
        Ok(plan)
    }

    /// Adopt the configured `provider` at startup: switch when the index is
    /// stamped for a different model, otherwise make it active and drop any
    /// shadow a crash left behind.
    pub fn reconcile_embedder(
        self: &Arc<Self>,
        provider: SharedProvider,
    ) -> Result<Option<SwitchPlan>, String> {
        let identity = provider.identity();
        if self.store.vector_stamp()?.as_ref() == Some(&identity) {
            if self.reembed.snapshot().state != super::ReembedState::Running {
                self.store.clear_shadow(&Self::write_context(), true)?;
            }
            self.adopt_published(provider);
            return Ok(None);
        }
        self.switch_embedder(provider).map(Some)
    }

    /// Serve recall with the published model while a configured switch resumes.
    pub fn adopt_published(&self, provider: SharedProvider) {
        if self.embedder.identity() != provider.identity() {
            self.store
                .adopt_published(&Self::write_context(), &self.embedder, provider);
            self.embedder
                .emit(&EmbedderEvent::Switched(self.embedder.identity()));
            self.rebuild_docs_vectors();
        }
        self.reembed.retain_local_models(&self.embedder);
        self.backfill_pending();
    }

    /// Queue every vector-pending item and entity again.
    pub fn backfill_pending(&self) -> usize {
        let ids = match self.store.pending_ids() {
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
                    let _ = self.store.clear_pending(&Self::write_context(), &id);
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
        let sections = self.docs_sections().to_vec();
        let spawned = std::thread::Builder::new()
            .name("docs-vector-build".into())
            .spawn(move || {
                if let Err(error) =
                    crate::docs_source::reembed_vectors(&data_dir, &embedder, &sections)
                {
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
            if matches!(event, EmbedderEvent::Progress(_)) {
                if let Some(manager) = manager.upgrade() {
                    manager.reembed.retain_local_models(&manager.embedder);
                }
            }
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
        self.store.pending_embed_text(id)
    }
}
