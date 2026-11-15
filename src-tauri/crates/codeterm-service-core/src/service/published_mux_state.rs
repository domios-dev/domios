use arc_swap::ArcSwap;
use bytes::Bytes;
use serde::Serialize;
use std::sync::Arc;

mod status_delta;

#[cfg(test)]
thread_local! { static JSON_ENCODINGS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

fn encode(value: &serde_json::Value) -> Result<Bytes, serde_json::Error> {
    #[cfg(test)]
    JSON_ENCODINGS.with(|count| count.set(count.get() + 1));
    serde_json::to_vec(value).map(Bytes::from)
}

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
        let json = encode(value.as_ref())?;
        Ok(Self {
            generation,
            latest: generation,
            json,
            value,
        })
    }
}

pub(crate) struct PublishedMuxState {
    current: ArcSwap<Snapshot>,
}

impl PublishedMuxState {
    pub(crate) fn new(generation: u64, value: &impl Serialize) -> Result<Self, serde_json::Error> {
        Ok(Self {
            current: ArcSwap::from_pointee(Snapshot::new(generation, value)?),
        })
    }

    pub(crate) fn publish_startup(
        &self,
        generation: u64,
        value: &impl Serialize,
    ) -> Result<(), serde_json::Error> {
        let next = Arc::new(Snapshot::new(generation, value)?);
        let _ = self.current.rcu(|current| {
            if generation > current.latest {
                next.clone()
            } else {
                current.clone()
            }
        });
        Ok(())
    }

    pub(super) fn read(&self) -> Bytes {
        self.current.load().json.clone()
    }

    pub(super) fn read_value(&self) -> serde_json::Value {
        self.current.load().value.as_ref().clone()
    }

    pub(crate) fn read_versioned(&self) -> (u64, Bytes) {
        let snapshot = self.current.load();
        (snapshot.generation, snapshot.json.clone())
    }

    pub(super) fn publish_change(
        &self,
        generation: u64,
        value: &impl Serialize,
    ) -> Result<Option<serde_json::Value>, serde_json::Error> {
        if generation <= self.current.load().latest {
            return Ok(None);
        }
        let value = Arc::new(serde_json::to_value(value)?);
        let mut encoded: Option<Arc<Snapshot>> = None;
        let mut error = None;
        let previous = self.current.rcu(|current| {
            if generation <= current.latest || error.is_some() {
                current.clone()
            } else if status_delta::equal_except(&value, &current.value, &["generation"]) {
                Arc::new(Snapshot {
                    generation: current.generation,
                    latest: generation,
                    json: current.json.clone(),
                    value: current.value.clone(),
                })
            } else {
                if let Some(encoded) = &encoded {
                    return encoded.clone();
                }
                match encode(&value) {
                    Ok(json) => {
                        let next = Arc::new(Snapshot {
                            generation,
                            latest: generation,
                            json,
                            value: value.clone(),
                        });
                        encoded = Some(next.clone());
                        next
                    }
                    Err(failure) => {
                        error = Some(failure);
                        current.clone()
                    }
                }
            }
        });
        if let Some(error) = error {
            return Err(error);
        }
        Ok((generation > previous.latest
            && !status_delta::equal_except(&value, &previous.value, &["generation"]))
        .then(|| status_delta::event(previous.generation, &previous.value, generation, &value)))
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
