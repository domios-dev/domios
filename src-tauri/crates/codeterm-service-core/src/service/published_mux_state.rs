use arc_swap::ArcSwap;
use bytes::Bytes;
use serde::Serialize;
use std::sync::Arc;

struct Snapshot {
    generation: u64,
    json: Bytes,
}

pub(super) struct PublishedMuxState {
    current: ArcSwap<Snapshot>,
}

impl PublishedMuxState {
    pub(super) fn new(generation: u64, value: &impl Serialize) -> Result<Self, serde_json::Error> {
        Ok(Self {
            current: ArcSwap::from_pointee(Snapshot {
                generation,
                json: serde_json::to_vec(value)?.into(),
            }),
        })
    }

    pub(super) fn read(&self) -> Bytes {
        self.current.load().json.clone()
    }

    pub(super) fn publish(
        &self,
        generation: u64,
        value: &impl Serialize,
    ) -> Result<bool, serde_json::Error> {
        let next = Arc::new(Snapshot {
            generation,
            json: serde_json::to_vec(value)?.into(),
        });
        let previous = self.current.rcu(|current| {
            if generation > current.generation {
                next.clone()
            } else {
                current.clone()
            }
        });
        Ok(generation > previous.generation)
    }
}

impl super::ServiceContext {
    pub fn published_mux_state(&self) -> Bytes {
        self.ext::<Arc<PublishedMuxState>>().read()
    }
}

#[cfg(test)]
mod tests;
