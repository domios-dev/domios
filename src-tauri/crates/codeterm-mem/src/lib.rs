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
pub mod manager_crud;
mod manager_graph;
mod manager_layout;
mod manager_queries;
mod manager_search;
mod manager_storage;
#[cfg(test)]
mod manager_tests;
mod reindex;

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub use extraction_authority::{
    BindOutcome, CommitAuthority, ExtractionAuthority, ExtractionCancelSink, GenerationId,
    RevokedGeneration,
};
pub use graph::{Entity, Relation};
pub use layers::WakeUpOptions;
pub use manager_crud::AreaForgetOutcome;
pub use reindex::{body_is_embeddable, compose_embed_text, MIN_EMBED_BODY_CHARS};
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
    pub(crate) conn: Arc<Mutex<Connection>>,
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
    pub fn open(data_dir: &std::path::Path) -> Result<Arc<Self>, String> {
        let manager = Self::open_with(
            data_dir,
            embedder::ActiveEmbedder::global(),
            Arc::new(embedder::SystemClock),
            Some(data_dir.to_path_buf()),
        )?;
        docs_source::start_background_build(data_dir);
        spawn_entity_embedding_backfill(manager.db_path.clone(), manager.embedder.clone());
        Ok(manager)
    }

    /// `open` with a caller-supplied embedder and clock, and no startup backfill.
    pub fn open_with(
        data_dir: &std::path::Path,
        embedder: Arc<embedder::ActiveEmbedder>,
        clock: Arc<dyn embedder::Clock>,
        docs_dir: Option<PathBuf>,
    ) -> Result<Arc<Self>, String> {
        let mem_dir = data_dir.join("mem");
        std::fs::create_dir_all(&mem_dir).map_err(|e| format!("create mem dir: {}", e))?;
        let blobs_dir = mem_dir.join("blobs");
        std::fs::create_dir_all(&blobs_dir).map_err(|e| format!("create blobs dir: {}", e))?;

        embeddings::ensure_extension_registered();

        let db_path = mem_dir.join("knowledge.db");
        let conn = Connection::open(&db_path).map_err(|e| format!("open knowledge.db: {}", e))?;
        // concurrent writers wait up to 5s instead of failing — required for multi-process access (daemon + `mem run-worker` subprocess share the DB).
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("set busy_timeout: {}", e))?;

        // WAL + synchronous=NORMAL pragmas come from store::init_schema.
        store::init_schema(&conn).map_err(|e| format!("init schema: {}", e))?;
        backend::sqlite::blueprint::cleanup::remove_v1(&conn, &blobs_dir)?;
        backend::sqlite::blueprint::metadata::initialize(&conn)?;

        if let Err(e) = embeddings::init_index(&conn, &embedder.identity()) {
            log::warn!(
                "[mem] vector index init failed: {} — semantic recall disabled",
                e
            );
        }

        let heavy_ops_permits = Arc::new(Semaphore::new(HEAVY_OPS_PERMITS));
        let layout = graph_layout::LayoutState::load(&mem_dir);
        let embed_queue = embed_queue::EmbedQueue::start(
            db_path.clone(),
            heavy_ops_permits.clone(),
            layout.cache.clone(),
            embedder.clone(),
        );

        let conn = Arc::new(Mutex::new(conn));
        let store = Arc::new(backend::SqliteMemStore::from_connection(
            conn.clone(),
            blobs_dir.clone(),
        ));
        let manager = Arc::new(Self {
            blueprint: Default::default(),
            store,
            conn,
            db_path,
            blobs_dir,
            skill_dir: data_dir.join("skills"),
            embed_queue,
            embedder,
            reembed: embedder::switch::ReembedTracker::new(clock),
            docs_dir,
            #[cfg(test)]
            docs_sections: Default::default(),
            heavy_ops_permits,
            layout,
            extraction: Arc::new(ExtractionAuthority::new()),
        });
        manager
            .reembed
            .restore(mem_dir.join("reembed-progress.json"));
        manager.subscribe_recovery();
        Ok(manager)
    }

    pub fn open_in_memory() -> Result<Arc<Self>, String> {
        embeddings::ensure_extension_registered();
        #[cfg(feature = "test-support")]
        let conn = test_support::memory_connection()?;
        #[cfg(not(feature = "test-support"))]
        let conn = Self::empty_memory_connection()?;
        Self::with_memory_connection(conn)
    }

    fn empty_memory_connection() -> Result<Connection, String> {
        let conn = Connection::open_in_memory().map_err(|e| format!("open :memory:: {}", e))?;
        store::init_schema(&conn).map_err(|e| format!("init schema: {}", e))?;
        backend::sqlite::blueprint::metadata::initialize(&conn)?;
        Ok(conn)
    }

    fn with_memory_connection(conn: Connection) -> Result<Arc<Self>, String> {
        let tmp = std::env::temp_dir().join(format!("mem-test-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(tmp.join("blobs")).ok();
        let layout = graph_layout::LayoutState::load(&tmp);
        #[cfg(test)]
        let embedder = {
            let model = embedder::local::default_model();
            let provider = embedder::testing::MockProvider::new(
                embedder::local::ID,
                model.model,
                model.dimension,
            );
            provider.set_down(true);
            embedder::ActiveEmbedder::new(provider)
        };
        #[cfg(not(test))]
        let embedder = embedder::ActiveEmbedder::global();
        let conn = Arc::new(Mutex::new(conn));
        let store = Arc::new(backend::SqliteMemStore::from_connection(
            conn.clone(),
            tmp.join("blobs"),
        ));
        Ok(Arc::new(Self {
            blueprint: Default::default(),
            store,
            conn,
            db_path: tmp.join("knowledge.db"),
            blobs_dir: tmp.join("blobs"),
            skill_dir: tmp.join("skills"),
            embed_queue: embed_queue::EmbedQueue::disabled(),
            embedder,
            reembed: embedder::switch::ReembedTracker::new(Arc::new(embedder::SystemClock)),
            docs_dir: None,
            #[cfg(test)]
            docs_sections: Default::default(),
            heavy_ops_permits: Arc::new(Semaphore::new(HEAVY_OPS_PERMITS)),
            layout,
            extraction: Arc::new(ExtractionAuthority::new()),
        }))
    }

    /// Offline / deterministic constructor for benches, evals, and tests
    /// outside the main test harness.
    ///
    /// Differences from [`MemManager::open`]:
    /// - No background embed queue worker thread is spawned. Any body that
    ///   would normally be enqueued for indexing is silently dropped.
    /// - No sqlite-vec extension warning if init fails (bench mode falls
    ///   back to FTS5-only recall by design).
    /// - `data_dir` is the final directory; the caller is responsible for
    ///   picking a tempdir path. `knowledge.db` + `blobs/` are created
    ///   inside it exactly like `open`.
    ///
    /// This constructor exists so the mem-recall bench runner can exercise
    /// the real insert / FTS search path without spinning up tokio tasks,
    /// calling any LLM, or requiring API keys / model weights.
    pub fn open_offline(data_dir: &std::path::Path) -> Result<Arc<Self>, String> {
        std::fs::create_dir_all(data_dir).map_err(|e| format!("create data dir: {}", e))?;
        let blobs_dir = data_dir.join("blobs");
        std::fs::create_dir_all(&blobs_dir).map_err(|e| format!("create blobs dir: {}", e))?;

        embeddings::ensure_extension_registered();

        let db_path = data_dir.join("knowledge.db");
        let conn = Connection::open(&db_path).map_err(|e| format!("open knowledge.db: {}", e))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("set busy_timeout: {}", e))?;

        store::init_schema(&conn).map_err(|e| format!("init schema: {}", e))?;
        backend::sqlite::blueprint::cleanup::remove_v1(&conn, &blobs_dir)?;
        backend::sqlite::blueprint::metadata::initialize(&conn)?;

        // Non-fatal: offline mode is allowed to run FTS5-only without vectors.
        let _ = embeddings::init_extension(&conn);

        let layout = graph_layout::LayoutState::load(data_dir);
        #[cfg(test)]
        let embedder = {
            let model = embedder::local::default_model();
            embedder::ActiveEmbedder::new(embedder::testing::MockProvider::new(
                embedder::local::ID,
                model.model,
                model.dimension,
            ))
        };
        #[cfg(not(test))]
        let embedder = embedder::ActiveEmbedder::global();
        let conn = Arc::new(Mutex::new(conn));
        let store = Arc::new(backend::SqliteMemStore::from_connection(
            conn.clone(),
            blobs_dir.clone(),
        ));
        Ok(Arc::new(Self {
            blueprint: Default::default(),
            store,
            conn,
            db_path,
            blobs_dir,
            skill_dir: data_dir.join("skills"),
            embed_queue: embed_queue::EmbedQueue::disabled(),
            embedder,
            reembed: embedder::switch::ReembedTracker::new(Arc::new(embedder::SystemClock)),
            docs_dir: None,
            #[cfg(test)]
            docs_sections: Default::default(),
            heavy_ops_permits: Arc::new(Semaphore::new(HEAVY_OPS_PERMITS)),
            layout,
            extraction: Arc::new(ExtractionAuthority::new()),
        }))
    }

    pub fn db_path(&self) -> &std::path::Path {
        &self.db_path
    }
    pub fn blobs_dir(&self) -> &std::path::Path {
        &self.blobs_dir
    }

    /// Flush WAL to the main database and truncate the WAL file. Returns
    /// `(busy, log_frames, checkpointed)` from `PRAGMA wal_checkpoint(TRUNCATE)`
    /// so the caller can log forensics around shutdown.
    ///
    /// Called from the updater's graceful shutdown so the new binary, which
    /// reopens the same db immediately after restart, sees a clean state with
    /// no orphaned WAL frames.
    pub fn checkpoint_and_close(&self) -> Result<(i64, i64, i64), String> {
        let conn = self.conn.lock();
        let row = conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
                let busy: i64 = r.get(0)?;
                let log_frames: i64 = r.get(1)?;
                let checkpointed: i64 = r.get(2)?;
                Ok((busy, log_frames, checkpointed))
            })
            .map_err(|e| format!("wal_checkpoint(TRUNCATE) failed: {}", e))?;
        Ok(row)
    }

    pub(crate) fn conn(&self) -> &parking_lot::Mutex<rusqlite::Connection> {
        &self.conn
    }

    /// Try to acquire the connection lock with a timeout. Returns `None` if
    /// another holder still owns it when the deadline expires — callers in
    /// request-handler paths translate that into HTTP 503 so the daemon
    /// stays responsive under heavy background load.
    #[cfg(test)]
    pub(crate) fn try_conn_for(
        &self,
        timeout: std::time::Duration,
    ) -> Option<parking_lot::MutexGuard<'_, rusqlite::Connection>> {
        self.conn.try_lock_for(timeout)
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

    /// Run a read-only SQL closure against the underlying connection.
    /// Used by sibling modules that need direct query access.
    pub(crate) fn with_conn<R>(&self, f: impl FnOnce(&rusqlite::Connection) -> R) -> R {
        let conn = self.conn.lock();
        f(&*conn)
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
    db_path: std::path::PathBuf,
    embedder: Arc<embedder::ActiveEmbedder>,
) {
    let result = spawn_entity_embedding_backfill_task(move || {
        let work = || -> Result<(), String> {
            let conn = Connection::open(&db_path)
                .map_err(|e| format!("open knowledge.db for entity backfill: {e}"))?;
            conn.busy_timeout(std::time::Duration::from_secs(5))
                .map_err(|e| format!("set entity backfill busy_timeout: {e}"))?;
            embeddings::init_index(&conn, &embedder.identity())
                .map_err(|e| format!("init vector index for entity backfill: {e}"))?;
            backfill_entity_embeddings(&conn, &embedder)
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

/// Idempotently embeds graph entities created before embed-on-create existed.
fn backfill_entity_embeddings(
    conn: &Connection,
    embedder: &embedder::ActiveEmbedder,
) -> Result<(), String> {
    let mut stmt = conn
        .prepare(
            "SELECT e.id, e.entity_type, e.properties_json
             FROM entities e
             LEFT JOIN mem_vectors v ON v.mem_id = e.id
             WHERE v.mem_id IS NULL",
        )
        .map_err(|e| format!("backfill prepare: {}", e))?;
    let rows: Vec<(String, String, String)> = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| format!("backfill query: {}", e))?
        .filter_map(Result::ok)
        .collect();

    if rows.is_empty() {
        log::info!("[mem.backfill] entity embeddings already up to date");
        return Ok(());
    }

    log::info!(
        "[mem.backfill] embedding {} entities missing from mem_vectors",
        rows.len()
    );
    let total = rows.len();
    let mut ok_count = 0usize;
    for (id, entity_type, props_json) in rows {
        let text = embeddings::embeddable_text_for_entity(&id, &entity_type, &props_json);
        match embeddings::index_with(conn, embedder, &id, &text) {
            Ok(_) => ok_count += 1,
            Err(e) => log::warn!("[mem.backfill] embed {} failed: {}", id, e),
        }
    }
    log::info!(
        "[mem.backfill] entity embedding backfill done: {}/{} OK",
        ok_count,
        total
    );
    Ok(())
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
