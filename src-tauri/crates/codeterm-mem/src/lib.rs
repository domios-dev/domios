//! Unified mem store — one SQLite, one CLI, one query path.

pub mod activity;
pub mod auto_extract;
#[cfg_attr(not(feature = "embeddings"), allow(dead_code))]
mod chunking;
pub mod decision;
pub mod dream_status;
pub mod dreaming;
pub mod embed_queue;
#[cfg_attr(not(feature = "embeddings"), allow(dead_code))]
mod embed_schedule;
pub mod embeddings;
pub mod extraction_authority;
pub mod file_sync;
pub mod graph;
pub mod graph_layout;
pub mod graph_repair;
pub mod ids;
pub mod layers;
pub mod learnings;
pub mod ports;
pub mod prompt_text;
pub mod rerank;
pub mod search;
pub mod seed;
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
#[cfg(feature = "embeddings")]
pub use reindex::compose_embed_text;
pub use reindex::{body_is_embeddable, MIN_EMBED_BODY_CHARS};
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
        .join(".codeterm")
        .join("fastembed_cache")
}

/// Cap on concurrent long-running mem operations (dreaming, extraction,
/// embedding). API search / recall / save / get never acquire this permit,
/// so they keep priority access to the connection lock.
const HEAVY_OPS_PERMITS: usize = 2;

pub struct MemManager {
    pub(crate) conn: Mutex<Connection>,
    pub(crate) db_path: PathBuf,
    pub(crate) blobs_dir: PathBuf,
    skill_dir: PathBuf,
    pub(crate) embed_queue: embed_queue::EmbedQueue,
    pub(crate) heavy_ops_permits: Arc<Semaphore>,
    pub(crate) layout: graph_layout::LayoutState,
    pub(crate) extraction: Arc<ExtractionAuthority>,
}

impl MemManager {
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
        let manager = Self::open_with_queue(data_dir, embed_queue::EmbedQueue::start)?;
        spawn_entity_embedding_backfill(manager.db_path.clone());
        Ok(manager)
    }

    /// `open` with a caller-supplied embedder and no entity backfill.
    #[cfg(test)]
    pub(crate) fn open_with_embedder(
        data_dir: &std::path::Path,
        embedder: embed_queue::Embedder,
    ) -> Result<Arc<Self>, String> {
        Self::open_with_queue(data_dir, |db_path, permits, layout| {
            embed_queue::EmbedQueue::start_with(db_path, permits, layout, embedder)
        })
    }

    fn open_with_queue(
        data_dir: &std::path::Path,
        start_queue: impl FnOnce(
            PathBuf,
            Arc<Semaphore>,
            Arc<graph_layout::cache::LayoutCache>,
        ) -> embed_queue::EmbedQueue,
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

        if let Err(e) = embeddings::init_extension(&conn) {
            log::warn!(
                "[mem] sqlite-vec init failed: {} — semantic recall disabled",
                e
            );
        }

        let heavy_ops_permits = Arc::new(Semaphore::new(HEAVY_OPS_PERMITS));
        let layout = graph_layout::LayoutState::load(&mem_dir);
        let embed_queue = start_queue(
            db_path.clone(),
            heavy_ops_permits.clone(),
            layout.cache.clone(),
        );

        Ok(Arc::new(Self {
            conn: Mutex::new(conn),
            db_path,
            blobs_dir,
            skill_dir: data_dir.join("skills"),
            embed_queue,
            heavy_ops_permits,
            layout,
            extraction: Arc::new(ExtractionAuthority::new()),
        }))
    }

    pub fn open_in_memory() -> Result<Arc<Self>, String> {
        let conn = Connection::open_in_memory().map_err(|e| format!("open :memory:: {}", e))?;
        store::init_schema(&conn).map_err(|e| format!("init schema: {}", e))?;
        let tmp = std::env::temp_dir().join(format!("mem-test-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(tmp.join("blobs")).ok();
        let layout = graph_layout::LayoutState::load(&tmp);
        Ok(Arc::new(Self {
            conn: Mutex::new(conn),
            db_path: tmp.join("knowledge.db"),
            blobs_dir: tmp.join("blobs"),
            skill_dir: tmp.join("skills"),
            embed_queue: embed_queue::EmbedQueue::disabled(),
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

        // Non-fatal: offline mode is allowed to run FTS5-only without vectors.
        let _ = embeddings::init_extension(&conn);

        let layout = graph_layout::LayoutState::load(data_dir);
        Ok(Arc::new(Self {
            conn: Mutex::new(conn),
            db_path,
            blobs_dir,
            skill_dir: data_dir.join("skills"),
            embed_queue: embed_queue::EmbedQueue::disabled(),
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

    pub fn conn(&self) -> &parking_lot::Mutex<rusqlite::Connection> {
        &self.conn
    }

    /// Try to acquire the connection lock with a timeout. Returns `None` if
    /// another holder still owns it when the deadline expires — callers in
    /// request-handler paths translate that into HTTP 503 so the daemon
    /// stays responsive under heavy background load.
    pub fn try_conn_for(
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

    /// Initialize the sqlite-vec extension and mem_vectors table.
    /// No-op when the `embeddings` feature is not compiled in.
    pub fn init_embeddings(&self) -> Result<(), String> {
        let conn = self.conn.lock();
        embeddings::init_extension(&conn)
    }

    /// Run a read-only SQL closure against the underlying connection.
    /// Used by sibling modules that need direct query access.
    pub fn with_conn<R>(&self, f: impl FnOnce(&rusqlite::Connection) -> R) -> R {
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
fn spawn_entity_embedding_backfill(db_path: std::path::PathBuf) {
    let result = spawn_entity_embedding_backfill_task(move || {
        let work = || -> Result<(), String> {
            let conn = Connection::open(&db_path)
                .map_err(|e| format!("open knowledge.db for entity backfill: {e}"))?;
            conn.busy_timeout(std::time::Duration::from_secs(5))
                .map_err(|e| format!("set entity backfill busy_timeout: {e}"))?;
            embeddings::init_extension(&conn)
                .map_err(|e| format!("init sqlite-vec for entity backfill: {e}"))?;
            backfill_entity_embeddings(&conn)
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
fn backfill_entity_embeddings(conn: &Connection) -> Result<(), String> {
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
        match embeddings::index(conn, &id, &text) {
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
