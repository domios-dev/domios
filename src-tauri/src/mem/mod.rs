//! Unified mem store — one SQLite, one CLI, one query path. See docs/plans/mem-architecture.md.

pub mod activity;
pub mod auto_extract;
pub mod dreaming;
pub mod embed_queue;
pub mod embeddings;
pub mod file_sync;
pub mod graph;
pub mod graph_layout;
pub mod graph_repair;
pub mod ids;
pub mod layers;
pub mod learnings;
pub mod rerank;
pub mod search;
pub mod seed;
pub mod store;
pub mod supervisor;
pub mod worker_pool;
pub mod workers;

mod manager_crud;
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

pub use graph::{Entity, Relation};
pub use layers::WakeUpOptions;
#[cfg(feature = "embeddings")]
pub use reindex::compose_embed_text;
pub use reindex::{body_is_embeddable, MIN_EMBED_BODY_CHARS};
pub use search::{SearchHit, SearchQuery};
pub use store::{MemItem, MemKind, MemScope, NewMemItem};

/// Cap on concurrent long-running mem operations (dreaming, extraction,
/// embedding). API search / recall / save / get never acquire this permit,
/// so they keep priority access to the connection lock.
const HEAVY_OPS_PERMITS: usize = 2;

pub struct MemManager {
    pub(super) conn: Mutex<Connection>,
    pub(super) db_path: PathBuf,
    pub(super) blobs_dir: PathBuf,
    pub(super) embed_queue: embed_queue::EmbedQueue,
    pub(super) heavy_ops_permits: Arc<Semaphore>,
    pub(super) layout: graph_layout::LayoutState,
}

impl MemManager {
    pub fn open(data_dir: &std::path::Path) -> Result<Arc<Self>, String> {
        let mem_dir = data_dir.join("mem");
        std::fs::create_dir_all(&mem_dir).map_err(|e| format!("create mem dir: {}", e))?;
        let blobs_dir = mem_dir.join("blobs");
        std::fs::create_dir_all(&blobs_dir).map_err(|e| format!("create blobs dir: {}", e))?;

        // sqlite-vec must register BEFORE Connection::open
        embeddings::ensure_extension_registered();

        let db_path = mem_dir.join("knowledge.db");
        let conn = Connection::open(&db_path).map_err(|e| format!("open knowledge.db: {}", e))?;
        // concurrent writers wait up to 5s instead of failing — required for multi-process access (daemon + `mem run-worker` subprocess share the DB).
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("set busy_timeout: {}", e))?;

        // WAL + synchronous=NORMAL pragmas come from store::init_schema.
        store::init_schema(&conn).map_err(|e| format!("init schema: {}", e))?;

        // non-fatal — FTS5 hybrid still works without vectors
        if let Err(e) = embeddings::init_extension(&conn) {
            log::warn!(
                "[mem] sqlite-vec init failed: {} — semantic recall disabled",
                e
            );
        }

        // Backfill: any entity row that has no mem_vectors entry (typically entities created before v1.1.23 wired auto-embed-on-create) gets embedded
        // now. Idempotent — entities already embedded are skipped. Best-effort: errors logged, never block startup.
        if let Err(e) = backfill_entity_embeddings(&conn) {
            log::warn!("[mem] entity-embedding backfill failed: {}", e);
        }

        let heavy_ops_permits = Arc::new(Semaphore::new(HEAVY_OPS_PERMITS));
        let layout = graph_layout::LayoutState::load(&mem_dir);
        let embed_queue = embed_queue::EmbedQueue::start(
            db_path.clone(),
            heavy_ops_permits.clone(),
            layout.cache.clone(),
        );

        Ok(Arc::new(Self {
            conn: Mutex::new(conn),
            db_path,
            blobs_dir,
            embed_queue,
            heavy_ops_permits,
            layout,
        }))
    }

    #[cfg(test)]
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
            embed_queue: embed_queue::EmbedQueue::disabled(),
            heavy_ops_permits: Arc::new(Semaphore::new(HEAVY_OPS_PERMITS)),
            layout,
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
            embed_queue: embed_queue::EmbedQueue::disabled(),
            heavy_ops_permits: Arc::new(Semaphore::new(HEAVY_OPS_PERMITS)),
            layout,
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

/// Embed every graph entity that doesn't already have a row in `mem_vectors`.
/// Called once on `MemManager::open` so daemon startup auto-backfills entities
/// created before v1.1.23 wired embed-on-create. Idempotent: existing vectors
/// are not re-computed; new ones are added; entities are skipped on embed
/// failure (logged) so one bad row can't block the whole backfill.
fn backfill_entity_embeddings(conn: &Connection) -> Result<(), String> {
    use rusqlite::params;
    // Pull entities lacking any mem_vectors row (matched on the canonical id — chunked vectors live under `<id>_chunk_N`, but graph-entity bodies
    // are tiny so we expect one row per id). LEFT JOIN to detect absence.
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
        let _ = params![]; // silence unused import warning if rusqlite path inactive
    }
    log::info!(
        "[mem.backfill] entity embedding backfill done: {}/{} OK",
        ok_count,
        total
    );
    Ok(())
}
