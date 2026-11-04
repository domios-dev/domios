use super::{EmbedError, StampedVector};
use parking_lot::{Condvar, Mutex};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

const MAX_FLIGHTS: usize = 4;
const MAX_CACHED: usize = 16;
const MAX_CACHE_BYTES: usize = 1024 * 1024;

type Key = (u64, [u8; 32]);

#[derive(Default)]
pub(super) struct QueryCache(Mutex<State>);

#[derive(Default)]
struct State {
    generation: u64,
    running: HashMap<Key, Arc<Flight>>,
    ready: VecDeque<(Key, StampedVector)>,
}

pub(super) enum Admission {
    Cached(StampedVector),
    Pending {
        key: Key,
        flight: Arc<Flight>,
        start: bool,
    },
}

#[derive(Default)]
pub(super) struct Flight {
    result: Mutex<Option<Result<StampedVector, EmbedError>>>,
    changed: Condvar,
}

impl QueryCache {
    pub(super) fn invalidate(&self) {
        let mut state = self.0.lock();
        state.generation = state.generation.wrapping_add(1);
        state.ready.clear();
    }

    pub(super) fn admit(&self, text: &str, budget: Duration) -> Option<Admission> {
        let digest = Sha256::digest(text.as_bytes()).into();
        let mut state = self.0.lock();
        let key = (state.generation, digest);
        if let Some((_, vector)) = state.ready.iter().find(|(cached, _)| *cached == key) {
            return Some(Admission::Cached(vector.clone()));
        }
        if budget.is_zero() {
            return None;
        }
        if let Some(flight) = state.running.get(&key) {
            return Some(Admission::Pending {
                key,
                flight: flight.clone(),
                start: false,
            });
        }
        if state.running.len() >= MAX_FLIGHTS {
            return None;
        }
        let flight = Arc::new(Flight::default());
        state.running.insert(key, flight.clone());
        Some(Admission::Pending {
            key,
            flight,
            start: true,
        })
    }

    pub(super) fn finish(
        &self,
        key: Key,
        flight: &Flight,
        result: Result<StampedVector, EmbedError>,
    ) {
        let mut state = self.0.lock();
        if let Ok(vector) = &result {
            if key.0 == state.generation
                && vector.vector.len() * size_of::<f32>() <= MAX_CACHE_BYTES
            {
                state.ready.push_back((key, vector.clone()));
                while state.ready.len() > MAX_CACHED
                    || state
                        .ready
                        .iter()
                        .map(|(_, v)| v.vector.len() * size_of::<f32>())
                        .sum::<usize>()
                        > MAX_CACHE_BYTES
                {
                    state.ready.pop_front();
                }
            }
        }
        state.running.remove(&key);
        *flight.result.lock() = Some(result);
        drop(state);
        flight.changed.notify_all();
    }
}

impl Flight {
    pub(super) fn wait(&self, budget: Duration) -> Option<StampedVector> {
        let deadline = Instant::now() + budget;
        let mut result = self.result.lock();
        loop {
            if let Some(result) = &*result {
                return result.as_ref().ok().cloned();
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            self.changed.wait_for(&mut result, remaining);
        }
    }
}

#[cfg(test)]
mod tests;
