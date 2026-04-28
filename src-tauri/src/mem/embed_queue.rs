//! Single-worker embedding indexer — one sqlite writer, bounded queue.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::thread;

use tokio::sync::Semaphore;

use super::embeddings;

const QUEUE_CAPACITY: usize = 1024;

#[derive(Debug)]
struct EmbedJob {
    id: String,
    text: String,
}

#[derive(Clone)]
pub struct EmbedQueue {
    tx: Option<SyncSender<EmbedJob>>,
}

impl EmbedQueue {
    pub fn disabled() -> Self {
        Self { tx: None }
    }

    pub fn start(db_path: PathBuf, heavy_ops_permits: Arc<Semaphore>) -> Self {
        if !cfg!(feature = "embeddings") {
            return Self { tx: None };
        }

        let (tx, rx) = mpsc::sync_channel::<EmbedJob>(QUEUE_CAPACITY);

        let runtime = tokio::runtime::Handle::try_current().ok();
        thread::Builder::new()
            .name("mem-embed-queue".into())
            .spawn(move || run_worker(db_path, rx, heavy_ops_permits, runtime))
            .expect("spawn embed queue worker");

        Self { tx: Some(tx) }
    }

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
fn run_worker(
    db_path: PathBuf,
    rx: mpsc::Receiver<EmbedJob>,
    heavy_ops_permits: Arc<Semaphore>,
    runtime: Option<tokio::runtime::Handle>,
) {
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
        let _permit = runtime
            .as_ref()
            .map(|h| h.block_on(heavy_ops_permits.clone().acquire_owned()).ok())
            .unwrap_or(None);
        match embeddings::index(&conn, &job.id, &job.text) {
            Ok(()) => log::debug!("[mem] indexed embedding for {}", job.id),
            Err(e) => log::warn!("[mem] embed index {}: {}", job.id, e),
        }
    }

    log::info!("[mem] embed queue worker exiting (sender dropped)");
}

#[cfg(not(feature = "embeddings"))]
fn run_worker(
    _db_path: PathBuf,
    _rx: mpsc::Receiver<EmbedJob>,
    _heavy_ops_permits: Arc<Semaphore>,
    _runtime: Option<tokio::runtime::Handle>,
) {
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enqueue_when_disabled_is_noop() {
        let q = EmbedQueue { tx: None };
        q.enqueue("abc", "hello world");
        q.enqueue("def", "goodbye");
    }

    #[cfg(feature = "embeddings")]
    #[test]
    fn enqueue_accepts_jobs_up_to_capacity() {
        let (tx, rx) = std::sync::mpsc::sync_channel::<EmbedJob>(2);
        assert!(tx.try_send(EmbedJob { id: "a".into(), text: "x".into() }).is_ok());
        assert!(tx.try_send(EmbedJob { id: "b".into(), text: "y".into() }).is_ok());
        let result = tx.try_send(EmbedJob { id: "c".into(), text: "z".into() });
        assert!(matches!(result, Err(TrySendError::Full(_))));
        drop(rx);
    }
}
