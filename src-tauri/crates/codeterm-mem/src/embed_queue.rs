//! Single-worker embedding indexer: one sqlite writer, per-item fair schedule.
//!
//! Items are split into chunks on submit and embedded one chunk at a time in
//! round-robin order (see `embed_schedule`), so a huge item cannot starve the
//! rest. Delete and replace cancel an item's pending chunks.

use crate::backend::{MemStore, WriteContext};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Condvar, Mutex};
use tokio::sync::Semaphore;

use super::embed_schedule::{EmbedSchedule, EmbedUnit, SubmitOutcome};
use super::embedder::index::InsertOutcome;
use super::embedder::{ActiveEmbedder, EmbedError};
use super::graph_layout::cache::LayoutCache;

const QUEUE_CAPACITY: usize = 1024;

#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize)]
pub struct QueueStatus {
    pub queued: usize,
    pub in_flight: usize,
    pub failed: u64,
    pub unavailable: bool,
}

#[derive(Default)]
struct QueueState {
    schedule: EmbedSchedule,
    busy: bool,
    closed: bool,
    failed: u64,
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
    pub fn status(&self, unavailable: bool) -> QueueStatus {
        let Some(shared) = self.shared() else {
            return QueueStatus {
                unavailable: true,
                ..QueueStatus::default()
            };
        };
        let state = shared.state.lock();
        QueueStatus {
            queued: state.schedule.queued_chunks(),
            in_flight: usize::from(state.busy),
            failed: state.failed,
            unavailable: unavailable || state.closed,
        }
    }

    pub fn disabled() -> Self {
        Self { handle: None }
    }

    pub fn start(
        store: Arc<dyn MemStore>,
        heavy_ops_permits: Arc<Semaphore>,
        layout: Arc<LayoutCache>,
        embedder: Arc<ActiveEmbedder>,
    ) -> Self {
        let shared = Arc::new(Shared::default());
        let worker_shared = shared.clone();
        let runtime = tokio::runtime::Handle::try_current().ok();
        std::thread::Builder::new()
            .name("mem-embed-queue".into())
            .spawn(move || {
                run_worker(
                    store,
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
            SubmitOutcome::Full => {
                state.failed += 1;
                log::warn!(
                    "[mem] embed queue full ({} items pending), dropping {}",
                    state.schedule.pending_items(),
                    id
                );
            }
        }
    }

    #[cfg(all(test, feature = "embeddings"))]
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
    super::embeddings::chunk_text(text)
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

fn run_worker(
    store: Arc<dyn MemStore>,
    shared: Arc<Shared>,
    heavy_ops_permits: Arc<Semaphore>,
    runtime: Option<tokio::runtime::Handle>,
    layout: Arc<LayoutCache>,
    embedder: Arc<ActiveEmbedder>,
) {
    while let Some(unit) = next_unit(&shared) {
        let _permit = runtime
            .as_ref()
            .and_then(|h| h.block_on(heavy_ops_permits.clone().acquire_owned()).ok());
        process_unit(store.as_ref(), &shared, &layout, &embedder, &unit);
        let mut state = shared.state.lock();
        state.schedule.finish(&unit);
        state.busy = false;
        if state.schedule.is_idle() {
            shared.idle.notify_all();
        }
    }

    log::info!("[mem] embed queue worker exiting (queue closed)");
}

fn process_unit(
    store: &dyn MemStore,
    shared: &Shared,
    layout: &LayoutCache,
    embedder: &ActiveEmbedder,
    unit: &EmbedUnit,
) {
    if !shared.state.lock().schedule.is_current(unit) {
        return;
    }
    let vector = match embedder.passage(&unit.text) {
        Ok(vector) => vector,
        Err(EmbedError::Unavailable(reason)) => {
            shared.state.lock().failed += 1;
            let dropped = shared.state.lock().schedule.abandon(unit);
            if let Err(error) =
                store.mark_pending(&WriteContext::system("embed-queue"), &unit.id, &reason)
            {
                log::warn!("[mem] embed {}: {error}", unit.id);
            }
            log::info!(
                "[mem] embed {} vector pending ({reason}); dropped {dropped} queued chunks",
                unit.id
            );
            return;
        }
        Err(error) => {
            shared.state.lock().failed += 1;
            shared.state.lock().schedule.abandon(unit);
            log::debug!(
                "[mem] embed {} chunk {}: {error}",
                unit.id,
                unit.chunk_index
            );
            return;
        }
    };
    if !shared.state.lock().schedule.is_current(unit) {
        return;
    }
    match store.insert_vector(
        &WriteContext::system("embed-queue"),
        &unit.id,
        unit.chunk_index,
        &vector,
    ) {
        Ok(InsertOutcome::Inserted) => {
            if unit.chunk_index == 0 {
                store.layout_place_stored(layout, &unit.id);
            }
            if unit.is_last {
                if let Err(error) =
                    store.clear_pending(&WriteContext::system("embed-queue"), &unit.id)
                {
                    log::warn!("[mem] embed {}: {error}", unit.id);
                }
                log::debug!("[mem] indexed embedding for {}", unit.id);
            }
        }
        Ok(InsertOutcome::ItemGone) => {
            let dropped = shared.state.lock().schedule.abandon(unit);
            log::info!(
                "[mem] embed skipped deleted item {} (dropped {dropped} pending chunks)",
                unit.id
            );
        }
        Err(e) => {
            shared.state.lock().failed += 1;
            log::warn!("[mem] embed index {}: {}", unit.id, e);
        }
    }
}

#[cfg(test)]
mod tests;
