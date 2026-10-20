use arc_swap::ArcSwap;
use bytes::Bytes;
use serde::Serialize;
use std::sync::Arc;

struct Snapshot {
    generation: u64,
    /// Newest generation offered, which may carry content identical to `json`.
    latest: u64,
    json: Bytes,
    content_start: usize,
}

impl Snapshot {
    fn new(generation: u64, value: &impl Serialize) -> Result<Self, serde_json::Error> {
        let json = Bytes::from(serde_json::to_vec(value)?);
        let prefix = format!("{{\"generation\":{generation}");
        let content_start = if json.starts_with(prefix.as_bytes()) {
            prefix.len()
        } else {
            0
        };
        Ok(Self {
            generation,
            latest: generation,
            json,
            content_start,
        })
    }

    fn content(&self) -> &[u8] {
        &self.json[self.content_start..]
    }
}

pub(super) struct PublishedMuxState {
    current: ArcSwap<Snapshot>,
}

impl PublishedMuxState {
    pub(super) fn new(generation: u64, value: &impl Serialize) -> Result<Self, serde_json::Error> {
        Ok(Self {
            current: ArcSwap::from_pointee(Snapshot::new(generation, value)?),
        })
    }

    pub(super) fn read(&self) -> Bytes {
        self.current.load().json.clone()
    }

    pub(super) fn read_versioned(&self) -> (u64, Bytes) {
        let snapshot = self.current.load();
        (snapshot.generation, snapshot.json.clone())
    }

    /// True only when `value` is newer and its content, generation excluded,
    /// differs from the published body; an identical one keeps the body and its
    /// generation but still fences off older publications.
    pub(super) fn publish(
        &self,
        generation: u64,
        value: &impl Serialize,
    ) -> Result<bool, serde_json::Error> {
        let next = Arc::new(Snapshot::new(generation, value)?);
        let previous = self.current.rcu(|current| {
            if generation <= current.latest {
                current.clone()
            } else if next.content() == current.content() {
                Arc::new(Snapshot {
                    generation: current.generation,
                    latest: generation,
                    json: current.json.clone(),
                    content_start: current.content_start,
                })
            } else {
                next.clone()
            }
        });
        Ok(generation > previous.latest && next.content() != previous.content())
    }
}

impl super::ServiceContext {
    pub fn published_mux_state_versioned(&self) -> (u64, Bytes) {
        self.ext::<Arc<PublishedMuxState>>().read_versioned()
    }

    pub fn published_mux_state(&self) -> Bytes {
        self.ext::<Arc<PublishedMuxState>>().read()
    }
}

#[cfg(test)]
mod tests;
