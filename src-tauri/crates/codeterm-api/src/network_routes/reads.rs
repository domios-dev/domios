use std::collections::HashMap;
use std::sync::{Arc, OnceLock, Weak};

use axum::http::StatusCode;
use parking_lot::Mutex;
use serde_json::Value;
use tokio::sync::watch;

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
pub(super) enum Kind {
    Network,
    Serve,
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
pub(super) struct Key {
    owner: usize,
    port: u16,
    kind: Kind,
}

impl Key {
    pub(super) fn new<T>(owner: &Arc<T>, port: u16, kind: Kind) -> Self {
        Self {
            owner: Arc::as_ptr(owner) as usize,
            port,
            kind,
        }
    }
}

type ReadResult = Result<Value, (StatusCode, &'static str)>;

struct Flight {
    result: watch::Receiver<Option<ReadResult>>,
}

#[derive(Default)]
pub(super) struct Reads {
    active: Mutex<HashMap<Key, Weak<Flight>>>,
}

pub(super) fn shared() -> &'static Reads {
    static READS: OnceLock<Reads> = OnceLock::new();
    READS.get_or_init(Reads::default)
}

impl Reads {
    pub(super) async fn read(
        &self,
        key: Key,
        work: impl FnOnce() -> Value + Send + 'static,
    ) -> ReadResult {
        let (flight, producer) = {
            let mut active = self.active.lock();
            active.retain(|_, flight| flight.strong_count() > 0);
            match active.get(&key).and_then(Weak::upgrade) {
                Some(flight) => (flight, None),
                None => {
                    let (sender, result) = watch::channel(None);
                    let flight = Arc::new(Flight { result });
                    active.insert(key, Arc::downgrade(&flight));
                    (flight, Some(sender))
                }
            }
        };
        if let Some(sender) = producer {
            let owner = flight.clone();
            tokio::spawn(async move {
                let result = tokio::task::spawn_blocking(work).await.map_err(|error| {
                    log::error!("[network] read worker failed: {error}");
                    (StatusCode::SERVICE_UNAVAILABLE, "Network read failed")
                });
                sender.send_replace(Some(result));
                drop(owner);
            });
        }
        let mut result = flight.result.clone();
        loop {
            let ready = result.borrow_and_update().clone();
            if let Some(ready) = ready {
                return ready;
            }
            result.changed().await.map_err(|_| {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Network read worker stopped",
                )
            })?;
        }
    }
}

#[cfg(test)]
mod tests;
