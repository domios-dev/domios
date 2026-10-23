use arc_swap::ArcSwap;
use bytes::Bytes;
use serde::Serialize;
use std::sync::Arc;

mod status_delta;

struct Snapshot {
    generation: u64,
    /// Newest generation offered, which may carry content identical to `json`.
    latest: u64,
    json: Bytes,
    value: Arc<serde_json::Value>,
}

impl Snapshot {
    fn new(generation: u64, value: &impl Serialize) -> Result<Self, serde_json::Error> {
        let value = Arc::new(serde_json::to_value(value)?);
        let json = Bytes::from(serde_json::to_vec(value.as_ref())?);
        Ok(Self {
            generation,
            latest: generation,
            json,
            value,
        })
    }

    fn same_content(&self, other: &Self) -> bool {
        status_delta::equal_except(&self.value, &other.value, &["generation"])
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

    pub(super) fn read_value(&self) -> serde_json::Value {
        self.current.load().value.as_ref().clone()
    }

    pub(super) fn read_versioned(&self) -> (u64, Bytes) {
        let snapshot = self.current.load();
        (snapshot.generation, snapshot.json.clone())
    }

    pub(super) fn publish_change(
        &self,
        generation: u64,
        value: &impl Serialize,
    ) -> Result<Option<serde_json::Value>, serde_json::Error> {
        let next = Arc::new(Snapshot::new(generation, value)?);
        let previous = self.current.rcu(|current| {
            if generation <= current.latest {
                current.clone()
            } else if next.same_content(current) {
                Arc::new(Snapshot {
                    generation: current.generation,
                    latest: generation,
                    json: current.json.clone(),
                    value: current.value.clone(),
                })
            } else {
                next.clone()
            }
        });
        Ok(
            (generation > previous.latest && !next.same_content(&previous)).then(|| {
                status_delta::event(
                    previous.generation,
                    &previous.value,
                    generation,
                    &next.value,
                )
            }),
        )
    }

    #[cfg(test)]
    pub(super) fn publish(
        &self,
        generation: u64,
        value: &impl Serialize,
    ) -> Result<bool, serde_json::Error> {
        self.publish_change(generation, value)
            .map(|event| event.is_some())
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
