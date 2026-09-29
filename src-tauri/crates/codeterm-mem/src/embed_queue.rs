//! Single-worker embedding indexer: one sqlite writer, per-item fair schedule.
//!
//! Items are split into chunks on submit and embedded one chunk at a time in
//! round-robin order (see `embed_schedule`), so a huge item cannot starve the
//! rest. Delete and replace cancel an item's pending chunks.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Condvar, Mutex};
use tokio::sync::Semaphore;

use super::embed_schedule::{EmbedSchedule, EmbedUnit, SubmitOutcome};
use super::graph_layout::cache::LayoutCache;

const QUEUE_CAPACITY: usize = 1024;

pub(crate) type Embedder = Arc<dyn Fn(&str) -> Result<Vec<f32>, String> + Send + Sync>;

#[derive(Default)]
struct QueueState {
    schedule: EmbedSchedule,
    busy: bool,
    closed: bool,
}

#[derive(Default)]
struct Shared {
    state: Mutex<QueueState>,
    work: Condvar,
    idle: Condvar,
}

impl Shared {
    fn close(&self) {
        self.state.lock().closed = true;
        self.work.notify_all();
    }
}

/// Last handle dropped closes the queue and lets the worker exit.
struct Handle(Arc<Shared>);

impl Drop for Handle {
    fn drop(&mut self) {
        self.0.close();
    }
}

#[derive(Clone)]
pub struct EmbedQueue {
    handle: Option<Arc<Handle>>,
}

impl EmbedQueue {
    pub fn disabled() -> Self {
        Self { handle: None }
    }

    pub fn start(
        db_path: PathBuf,
        heavy_ops_permits: Arc<Semaphore>,
        layout: Arc<LayoutCache>,
    ) -> Self {
        if !cfg!(feature = "embeddings") {
            return Self::disabled();
        }
        Self::start_with(
            db_path,
            heavy_ops_permits,
            layout,
            Arc::new(|text: &str| super::embeddings::embed(text)),
        )
    }

    #[cfg_attr(not(feature = "embeddings"), allow(dead_code))]
    pub(crate) fn start_with(
        db_path: PathBuf,
        heavy_ops_permits: Arc<Semaphore>,
        layout: Arc<LayoutCache>,
        embedder: Embedder,
    ) -> Self {
        let shared = Arc::new(Shared::default());
        let worker_shared = shared.clone();
        let runtime = tokio::runtime::Handle::try_current().ok();
        std::thread::Builder::new()
            .name("mem-embed-queue".into())
            .spawn(move || {
                run_worker(
                    db_path,
                    worker_shared,
                    heavy_ops_permits,
                    runtime,
                    layout,
                    embedder,
                )
            })
            .expect("spawn embed queue worker");
        Self {
            handle: Some(Arc::new(Handle(shared))),
        }
    }

    fn shared(&self) -> Option<&Arc<Shared>> {
        self.handle.as_ref().map(|handle| &handle.0)
    }

    /// Queue `text` for `id`, superseding any pending chunks from an earlier save.
    pub fn enqueue(&self, id: impl Into<String>, text: impl Into<String>) {
        let Some(shared) = self.shared() else { return };
        let id = id.into();
        let chunks = chunks_for(&text.into());
        let chunk_count = chunks.len();
        let mut state = shared.state.lock();
        if state.closed {
            log::warn!("[mem] embed queue worker gone, dropping {id}");
            return;
        }
        match state.schedule.submit(&id, chunks, QUEUE_CAPACITY) {
            SubmitOutcome::Queued {
                generation,
                superseded,
            } => {
                log::info!(
                    "[mem] embed queued {id} chunks={chunk_count} generation={generation} superseded={superseded}"
                );
                drop(state);
                shared.work.notify_one();
            }
            SubmitOutcome::Full => log::warn!(
                "[mem] embed queue full ({} items pending), dropping {}",
                state.schedule.pending_items(),
                id
            ),
        }
    }

    #[cfg(test)]
    pub(crate) fn submit_chunks(&self, id: &str, chunks: Vec<String>) {
        let shared = self.shared().expect("embed queue enabled");
        shared.state.lock().schedule.submit(id, chunks, usize::MAX);
        shared.work.notify_one();
    }

    /// Drop every pending chunk of `id`. Call before deleting the row; a chunk
    /// already being embedded is refused at insert because its row is gone.
    pub fn cancel(&self, id: &str) {
        let Some(shared) = self.shared() else { return };
        let dropped = shared.state.lock().schedule.cancel(id);
        if dropped > 0 {
            log::info!("[mem] embed queue cancelled {dropped} pending chunks of {id}");
        }
    }

    /// Block until nothing is pending or in flight; `false` when `deadline` expires first.
    pub fn wait_idle(&self, deadline: Duration) -> bool {
        let Some(shared) = self.shared() else {
            return true;
        };
        let mut state = shared.state.lock();
        let result = shared.idle.wait_while_for(
            &mut state,
            |state| state.busy || !state.schedule.is_idle(),
            deadline,
        );
        !result.timed_out()
    }
}

fn chunks_for(text: &str) -> Vec<String> {
    #[cfg(feature = "embeddings")]
    {
        super::embeddings::chunk_text(text)
    }
    #[cfg(not(feature = "embeddings"))]
    {
        vec![text.to_string()]
    }
}

/// Take the next unit, or `None` once the queue is closed and drained.
fn next_unit(shared: &Shared) -> Option<EmbedUnit> {
    let mut state = shared.state.lock();
    loop {
        if let Some(unit) = state.schedule.next_unit() {
            state.busy = true;
            return Some(unit);
        }
        state.busy = false;
        shared.idle.notify_all();
        if state.closed {
            return None;
        }
        shared.work.wait(&mut state);
    }
}

#[cfg(feature = "embeddings")]
fn run_worker(
    db_path: PathBuf,
    shared: Arc<Shared>,
    heavy_ops_permits: Arc<Semaphore>,
    runtime: Option<tokio::runtime::Handle>,
    layout: Arc<LayoutCache>,
    embedder: Embedder,
) {
    super::embeddings::ensure_extension_registered();
    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => {
            let _ = c.busy_timeout(Duration::from_secs(5));
            c
        }
        Err(e) => {
            log::warn!("[mem] embed worker: open {}: {}", db_path.display(), e);
            let mut state = shared.state.lock();
            state.closed = true;
            state.schedule = EmbedSchedule::default();
            shared.idle.notify_all();
            return;
        }
    };

    log::info!("[mem] embed queue worker started on {}", db_path.display());

    while let Some(unit) = next_unit(&shared) {
        let _permit = runtime
            .as_ref()
            .and_then(|h| h.block_on(heavy_ops_permits.clone().acquire_owned()).ok());
        process_unit(&conn, &shared, &layout, &embedder, &unit);
        shared.state.lock().schedule.finish(&unit);
    }

    log::info!("[mem] embed queue worker exiting (queue closed)");
}

#[cfg(feature = "embeddings")]
fn process_unit(
    conn: &rusqlite::Connection,
    shared: &Shared,
    layout: &LayoutCache,
    embedder: &Embedder,
    unit: &EmbedUnit,
) {
    if !shared.state.lock().schedule.is_current(unit) {
        return;
    }
    let vector = match embedder(&unit.text) {
        Ok(vector) => vector,
        Err(e) => {
            log::warn!("[mem] embed {} chunk {}: {}", unit.id, unit.chunk_index, e);
            return;
        }
    };
    if !shared.state.lock().schedule.is_current(unit) {
        return;
    }
    match super::embeddings::insert_vector_if_item_live(conn, &unit.id, unit.chunk_index, &vector) {
        Ok(true) => {
            if unit.chunk_index == 0 {
                place_in_layout(conn, layout, &unit.id);
            }
            if unit.is_last {
                log::debug!("[mem] indexed embedding for {}", unit.id);
            }
        }
        Ok(false) => {
            let dropped = shared.state.lock().schedule.abandon(unit);
            log::info!(
                "[mem] embed skipped deleted item {} (dropped {dropped} pending chunks)",
                unit.id
            );
        }
        Err(e) => log::warn!("[mem] embed index {}: {}", unit.id, e),
    }
}

#[cfg(feature = "embeddings")]
fn place_in_layout(conn: &rusqlite::Connection, layout: &LayoutCache, id: &str) {
    if layout.is_empty() {
        return;
    }
    let blob: Option<Vec<u8>> = conn
        .query_row(
            "SELECT embedding FROM mem_vectors WHERE mem_id = ?1",
            rusqlite::params![id],
            |r| r.get::<_, Vec<u8>>(0),
        )
        .ok();
    if let Some(blob) = blob {
        let vec = super::graph_layout::placement::decode_vector(&blob);
        match super::graph_layout::placement::place_incremental(conn, layout, id, &vec) {
            Ok(_) => {}
            Err(e) => log::warn!("[graph_layout] incremental place {id}: {e}"),
        }
    }
}

#[cfg(not(feature = "embeddings"))]
fn run_worker(
    _db_path: PathBuf,
    shared: Arc<Shared>,
    _heavy_ops_permits: Arc<Semaphore>,
    _runtime: Option<tokio::runtime::Handle>,
    _layout: Arc<LayoutCache>,
    _embedder: Embedder,
) {
    while let Some(unit) = next_unit(&shared) {
        shared.state.lock().schedule.finish(&unit);
    }
}

#[cfg(test)]
mod tests;
