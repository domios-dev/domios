//! Unified mem store — one SQLite, one CLI, one query path.

pub mod activity;
pub mod auto_extract;
pub mod backend;
pub mod blueprint_library;
pub mod blueprint_wl;
mod chunking;
pub mod curation;
pub mod decision;
pub mod docs_source;
pub mod dream_status;
pub mod dreaming;
pub mod embed_queue;
mod embed_schedule;
pub mod embedder;
#[cfg(feature = "embeddings")]
mod embedding_model;
#[cfg(feature = "embeddings")]
mod embedding_worker;
pub mod embeddings;
pub mod extraction_authority;
pub mod extraction_retry;
pub mod file_sync;
pub mod graph;
pub mod graph_layout;
pub mod graph_repair;
pub mod ids;
pub mod layers;
pub mod learnings;
#[cfg(feature = "embeddings")]
mod model_files;
pub mod ports;
pub mod precompact_purge;
pub mod prompt_text;
pub mod rerank;
pub mod search;
pub mod seed;
pub mod solution_metadata;
mod solution_search;
pub mod store;
pub mod supervisor;
pub mod worker_agent;
pub mod worker_pool;
pub mod workers;

#[cfg(test)]
mod extraction_commit_tests;
mod manager_construction;
pub mod manager_crud;
mod manager_graph;
mod manager_layout;
mod manager_queries;
mod manager_search;
mod manager_storage;
#[cfg(test)]
#[path = "tests_manager.rs"]
mod manager_tests;
mod reindex;

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub use extraction_authority::{
    BindOutcome, CommitAuthority, ExtractionAuthority, ExtractionCancelSink, GenerationId,
    RevokedGeneration,
};
pub use graph::{Entity, Relation};
pub use layers::WakeUpOptions;
pub use manager_crud::AreaForgetOutcome;
pub use reindex::{
    body_is_embeddable, compose_embed_text, compose_item_embed_text, MIN_EMBED_BODY_CHARS,
};
pub use search::{SearchHit, SearchQuery};
pub use store::{MemItem, MemKind, MemScope, NewMemItem};

pub fn model_cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("CODETERM_EMBED_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(codeterm_paths::dir_name())
        .join("fastembed_cache")
}

/// Cap on concurrent long-running mem operations (dreaming, extraction,
/// embedding). API search / recall / save / get never acquire this permit,
/// so they keep priority access to the connection lock.
const HEAVY_OPS_PERMITS: usize = 2;

pub struct MemManager {
    blueprint: blueprint_library::services::BlueprintServices,
    pub(crate) store: Arc<dyn backend::MemStore>,
    pub(crate) db_path: PathBuf,
    pub(crate) blobs_dir: PathBuf,
    skill_dir: PathBuf,
    pub(crate) embed_queue: embed_queue::EmbedQueue,
    pub(crate) embedder: Arc<embedder::ActiveEmbedder>,
    pub(crate) reembed: embedder::switch::ReembedTracker,
    pub(crate) docs_dir: Option<PathBuf>,
    #[cfg(test)]
    pub(crate) docs_sections: std::sync::OnceLock<Vec<docs_source::DocSection>>,
    pub(crate) heavy_ops_permits: Arc<Semaphore>,
    pub(crate) layout: graph_layout::LayoutState,
    pub(crate) extraction: Arc<ExtractionAuthority>,
}

impl MemManager {
    pub(crate) fn write_context() -> backend::WriteContext {
        match store::scoped_author() {
            Some(author) => backend::WriteContext::new(author, store::current_changed_by()),
            None => backend::WriteContext::unscoped(store::current_changed_by()),
        }
    }

    pub fn extraction_authority(&self) -> &Arc<ExtractionAuthority> {
        &self.extraction
    }

    /// Refuse the commit when the ambient write origin resolves to a generation
    /// whose source was deleted. Checked on every write at this boundary, so a
    /// generation that already started executing still cannot land.
    pub(crate) fn guard_commit(&self) -> Result<(), String> {
        let authority = self
            .extraction
            .authorize_worker(store::current_write_worker().as_deref());
        match authority.refusal() {
            Some(reason) => Err(reason),
            None => Ok(()),
        }
    }
}

impl MemManager {
    pub fn db_path(&self) -> &std::path::Path {
        &self.db_path
    }
    pub fn blobs_dir(&self) -> &std::path::Path {
        &self.blobs_dir
    }
    pub fn checkpoint_and_close(&self) -> Result<(i64, i64, i64), String> {
        self.store.checkpoint()
    }

    /// Acquire one of `HEAVY_OPS_PERMITS` slots. Holders run the long-running
    /// background jobs (dreaming, extraction batch, embed queue); API
    /// request handlers never take this permit, so they keep priority access
    /// to the connection. Drop the returned guard to release.
    pub async fn acquire_heavy_permit(&self) -> OwnedSemaphorePermit {
        self.heavy_ops_permits
            .clone()
            .acquire_owned()
            .await
            .expect("heavy_ops semaphore closed")
    }

    /// Handle to the same semaphore for contexts that need blocking acquire
    /// (e.g. the embed-queue sync worker thread).
    pub fn heavy_ops_permits(&self) -> Arc<Semaphore> {
        self.heavy_ops_permits.clone()
    }

    pub(crate) fn queue_embed(&self, id: &str, body: &str) {
        self.embed_queue.enqueue(id.to_string(), body.to_string());
    }

    /// Initialize the vector index for the active embedder.
    pub fn init_embeddings(&self) -> Result<(), String> {
        self.store
            .init_vectors(&Self::write_context(), &self.embedder.identity())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemStats {
    pub total_items: u64,
    pub by_kind: std::collections::HashMap<String, u64>,
    pub total_relations: u64,
    pub total_entities: u64,
    #[serde(default)]
    pub total_vectors: u64,
}

/// Starts entity backfill without putting model inference on the startup caller.
fn spawn_entity_embedding_backfill(
    store: Arc<dyn backend::MemStore>,
    embedder: Arc<embedder::ActiveEmbedder>,
) {
    let result = spawn_entity_embedding_backfill_task(move || {
        let work = || -> Result<(), String> {
            let store = store.background_store()?;
            store.init_vectors(
                &backend::WriteContext::system("entity-backfill"),
                &embedder.identity(),
            )?;
            for (id, text) in store.missing_entity_vectors()? {
                let vectors = embedder.passages(&embeddings::chunk_text(&text));
                if let Err(error) = store.store_vectors(
                    &backend::WriteContext::system("entity-backfill"),
                    &id,
                    vectors,
                ) {
                    log::warn!("[mem.backfill] embed {id} failed: {error}");
                }
            }
            Ok(())
        };
        if let Err(error) = work() {
            log::warn!("[mem] entity-embedding backfill failed: {error}");
        }
    });
    if let Err(error) = result {
        log::warn!("[mem] failed to start entity-embedding backfill: {error}");
    }
}

fn spawn_entity_embedding_backfill_task(
    task: impl FnOnce() + Send + 'static,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("mem-entity-backfill".into())
        .spawn(task)
}

#[cfg(test)]
mod startup_backfill_tests {
    use super::spawn_entity_embedding_backfill_task;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn entity_backfill_cannot_hold_the_startup_caller() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let worker = spawn_entity_embedding_backfill_task(move || {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        })
        .unwrap();

        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        release_tx.send(()).unwrap();
        worker.join().unwrap();
    }
}

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
