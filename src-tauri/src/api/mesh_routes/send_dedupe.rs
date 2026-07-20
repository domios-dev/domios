//! Bounded single-flight dedupe for mesh delivery ids (#240).
//!
//! A logical mesh send carries a stable opaque `delivery_id`. The target keys a
//! recent-delivery window by (authenticated sender instance, delivery_id) and
//! admits the local dispatch exactly once: concurrent duplicates wait on the
//! in-flight leader and replay its optimistic response. Bounded by count AND TTL
//! so it can never become an unbounded-growth bug (cf. #237).

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

use crate::mesh::types::RelayResponse;

/// Hard ceiling on live delivery entries. The safety net against unbounded
/// growth: completed entries beyond this are evicted oldest-first.
pub const DEDUPE_CAPACITY: usize = 512;

/// How long a delivery id stays deduped. Mirrors the mesh nonce replay window.
/// Within the TTL a duplicate always replays; past it the id may re-dispatch.
pub const DEDUPE_TTL_MS: u64 = 300_000;

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Dedupe key: authenticated sender instance + opaque delivery id, NUL-joined.
/// NUL can't appear in either component, so distinct pairs never collide and a
/// peer can never dedupe against another peer's id.
pub fn dedupe_key(sender_id: &str, delivery_id: &str) -> String {
    format!("{sender_id}\u{0}{delivery_id}")
}

/// One logical delivery: in-flight until the leader completes it, then a cached
/// optimistic response every duplicate replays.
pub struct DeliveryEntry {
    response: Mutex<Option<RelayResponse>>,
    ready: tokio::sync::Notify,
    created_ms: u64,
}

impl DeliveryEntry {
    fn new(created_ms: u64) -> Self {
        Self {
            response: Mutex::new(None),
            ready: tokio::sync::Notify::new(),
            created_ms,
        }
    }

    fn is_complete(&self) -> bool {
        self.response
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some()
    }

    fn is_expired(&self, now: u64) -> bool {
        now.saturating_sub(self.created_ms) > DEDUPE_TTL_MS
    }

    /// Record the leader's optimistic response and wake every waiter.
    pub fn complete(&self, response: RelayResponse) {
        *self.response.lock().unwrap_or_else(|p| p.into_inner()) = Some(response);
        self.ready.notify_waiters();
    }

    /// Await the leader's response. Registers the wakeup BEFORE re-checking so a
    /// completion racing this call is never lost.
    pub async fn wait(&self) -> RelayResponse {
        loop {
            let notified = self.ready.notified();
            if let Some(response) = self
                .response
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone()
            {
                return response;
            }
            notified.await;
        }
    }
}

#[derive(Default)]
struct Store {
    order: VecDeque<String>,
    values: HashMap<String, Arc<DeliveryEntry>>,
}

impl Store {
    fn acquire(&mut self, key: String, now: u64) -> (Arc<DeliveryEntry>, bool) {
        self.purge_expired(now);
        if let Some(entry) = self.values.get(&key) {
            return (entry.clone(), false);
        }
        let entry = Arc::new(DeliveryEntry::new(now));
        self.order.push_back(key.clone());
        self.values.insert(key, entry.clone());
        self.evict_to_capacity();
        (entry, true)
    }

    /// Drop completed entries past their TTL. Order is insertion (creation) time,
    /// so the expired ones are a prefix; stop at the first live or in-flight one.
    /// An in-flight entry is never dropped — a waiter may be parked on it.
    fn purge_expired(&mut self, now: u64) {
        while let Some(front) = self.order.front() {
            match self.values.get(front) {
                Some(entry) if entry.is_expired(now) => {
                    if !entry.is_complete() {
                        break;
                    }
                    if let Some(key) = self.order.pop_front() {
                        self.values.remove(&key);
                    }
                }
                Some(_) => break,
                None => {
                    self.order.pop_front();
                }
            }
        }
    }

    fn forget(&mut self, key: &str) {
        if self.values.remove(key).is_some() {
            self.order.retain(|k| k != key);
        }
    }

    /// Hard ceiling: evict oldest completed entries until at capacity. In-flight
    /// entries are never evicted (single-flight safety), so a pure in-flight
    /// storm is bounded only by concurrent request count, not this cap.
    fn evict_to_capacity(&mut self) {
        while self.order.len() > DEDUPE_CAPACITY {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if self.values.get(&oldest).is_some_and(|e| e.is_complete()) {
                self.values.remove(&oldest);
            } else {
                self.order.push_back(oldest);
                break;
            }
        }
    }
}

/// Process-wide window of recent mesh deliveries.
pub struct MeshDeliveryDedupe {
    store: Mutex<Store>,
}

impl MeshDeliveryDedupe {
    pub fn new() -> Self {
        Self {
            store: Mutex::new(Store::default()),
        }
    }

    pub fn global() -> &'static MeshDeliveryDedupe {
        static GLOBAL: OnceLock<MeshDeliveryDedupe> = OnceLock::new();
        GLOBAL.get_or_init(MeshDeliveryDedupe::new)
    }

    /// Admit a delivery. `true` = this caller owns the dispatch and must
    /// `complete` the entry; `false` = a duplicate that must `wait`.
    pub fn acquire(&self, key: String, now: u64) -> (Arc<DeliveryEntry>, bool) {
        self.store
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .acquire(key, now)
    }

    /// Drop a key so a later retry re-dispatches. Used when the outcome was
    /// definitively-not-delivered: the taxonomy declared it safe to retry, so
    /// the window must not swallow that retry.
    pub fn forget(&self, key: &str) {
        self.store
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .forget(key);
    }
}

impl Default for MeshDeliveryDedupe {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
