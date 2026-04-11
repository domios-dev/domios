//! Single-threaded embedding indexer backed by a bounded channel.
//!
//! Every `save()` used to spawn its own `std::thread::spawn` + fresh SQLite
//! connection to index one item's embedding. On a 189-item bulk load that's
//! 189 threads fighting for the knowledge.db write lock. This module
//! replaces that with one worker thread that owns its own connection and
//! drains a bounded queue. `save()` only enqueues, never blocks on
//! embedding work.
//!
//! No-op when the `embeddings` feature is off — [`EmbedQueue::start`]
//! returns a disabled instance whose `enqueue` does nothing.

use std::path::PathBuf;
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::thread;

use super::embeddings;

/// Bounded queue capacity. Backpressure: if the worker falls more than this
/// many items behind, new enqueues drop with a warning. 1024 is ~5× a full
/// 189-item seed; in practice save() calls never exceed it.
const QUEUE_CAPACITY: usize = 1024;

/// One embedding job. `id` identifies the mem_item row, `text` is what we
/// feed into the local fastembed model.
#[derive(Debug)]
struct EmbedJob {
    id: String,
    text: String,
}

/// Handle to the background indexer. Cheap to clone — the tx is an
/// `Arc<SyncSender>` internally.
#[derive(Clone)]
pub struct EmbedQueue {
    tx: Option<SyncSender<EmbedJob>>,
}

impl EmbedQueue {
    /// Disabled handle whose `enqueue` is a no-op. Used for in-memory test
    /// instances that don't run the worker.
    pub fn disabled() -> Self {
        Self { tx: None }
    }

    /// Spawn the worker and return a handle. When the `embeddings` feature
    /// is off, returns a disabled instance whose [`enqueue`] is a no-op.
    ///
    /// The worker owns its own `rusqlite::Connection` opened on `db_path`.
    /// It exits cleanly when the sender drops — i.e. when [`MemManager`]
    /// drops.
    pub fn start(db_path: PathBuf) -> Self {
        if !cfg!(feature = "embeddings") {
            return Self { tx: None };
        }

        let (tx, rx) = mpsc::sync_channel::<EmbedJob>(QUEUE_CAPACITY);

        thread::Builder::new()
            .name("mem-embed-queue".into())
            .spawn(move || run_worker(db_path, rx))
            .expect("spawn embed queue worker");

        Self { tx: Some(tx) }
    }

    /// Enqueue a new item for background vectorization. Non-blocking:
    /// drops the job with a warning if the queue is full or the worker
    /// has died. Never panics, never blocks `save()`.
    pub fn enqueue(&self, id: impl Into<String>, text: impl Into<String>) {
        let Some(ref tx) = self.tx else { return };
        let job = EmbedJob {
            id: id.into(),
            text: text.into(),
        };
        match tx.try_send(job) {
            Ok(()) => {}
            Err(TrySendError::Full(j)) => {
                log::warn!(
                    "[mem] embed queue full ({}), dropping {} — worker is {} items behind",
                    QUEUE_CAPACITY,
                    j.id,
                    QUEUE_CAPACITY
                );
            }
            Err(TrySendError::Disconnected(j)) => {
                log::warn!("[mem] embed queue worker gone, dropping {}", j.id);
            }
        }
    }
}

#[cfg(feature = "embeddings")]
fn run_worker(db_path: PathBuf, rx: mpsc::Receiver<EmbedJob>) {
    embeddings::ensure_extension_registered();
    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => {
            let _ = c.busy_timeout(std::time::Duration::from_secs(5));
            c
        }
        Err(e) => {
            log::warn!("[mem] embed worker: open {}: {}", db_path.display(), e);
            return;
        }
    };

    log::info!("[mem] embed queue worker started on {}", db_path.display());

    for job in rx.iter() {
        match embeddings::index(&conn, &job.id, &job.text) {
            Ok(()) => log::debug!("[mem] indexed embedding for {}", job.id),
            Err(e) => log::warn!("[mem] embed index {}: {}", job.id, e),
        }
    }

    log::info!("[mem] embed queue worker exiting (sender dropped)");
}

#[cfg(not(feature = "embeddings"))]
fn run_worker(_db_path: PathBuf, _rx: mpsc::Receiver<EmbedJob>) {
    // no-op stub — EmbedQueue::start returns tx=None and this is never called
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enqueue_when_disabled_is_noop() {
        // Construct a disabled queue manually (don't start the worker).
        let q = EmbedQueue { tx: None };
        q.enqueue("abc", "hello world");
        q.enqueue("def", "goodbye");
        // No panic, no error, no thread started.
    }

    #[cfg(feature = "embeddings")]
    #[test]
    fn enqueue_accepts_jobs_up_to_capacity() {
        // We can't actually start a worker in tests (embeddings feature
        // needs the onnx model download). Build a raw sync_channel and
        // push into it directly to validate the bounded-queue semantics.
        let (tx, rx) = std::sync::mpsc::sync_channel::<EmbedJob>(2);
        assert!(tx.try_send(EmbedJob { id: "a".into(), text: "x".into() }).is_ok());
        assert!(tx.try_send(EmbedJob { id: "b".into(), text: "y".into() }).is_ok());
        // Third send should block — try_send returns Full.
        let result = tx.try_send(EmbedJob { id: "c".into(), text: "z".into() });
        assert!(matches!(result, Err(TrySendError::Full(_))));
        drop(rx);
    }
}
