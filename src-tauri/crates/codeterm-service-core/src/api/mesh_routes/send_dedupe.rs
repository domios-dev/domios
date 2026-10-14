//! Bounded single-flight dedupe for mesh delivery ids (#240): one dispatch per
//! (authenticated sender, delivery_id); duplicates replay; count + TTL bounded.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

use crate::mesh::types::RelayResponse;

/// Hard ceiling on live entries; completed ones beyond it evict oldest-first.
pub const DEDUPE_CAPACITY: usize = 512;

/// Dedupe lifetime; mirrors the mesh nonce window. Within it a duplicate replays.
pub const DEDUPE_TTL_MS: u64 = 300_000;

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Dedupe key: sender instance + delivery id, NUL-joined so distinct pairs never
/// collide and one peer can't dedupe against another's id.
pub fn dedupe_key(sender_id: &str, delivery_id: &str) -> String {
    format!("{sender_id}\u{0}{delivery_id}")
}

/// Outcome of admitting a delivery into the window.
pub enum Admission {
    /// Caller owns the single dispatch; `complete` the entry when done.
    Owner(Arc<DeliveryEntry>),
    /// A duplicate of an in-flight/cached delivery; `wait` and replay.
    Duplicate(Arc<DeliveryEntry>),
    /// Window saturated by in-flight leaders; nothing dispatched, safe to retry.
    AtCapacity,
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

    /// Await the leader's response; registers the wakeup before re-checking so a
    /// racing completion is never lost.
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
    fn acquire(&mut self, key: String, now: u64) -> Admission {
        self.purge_expired(now);
        if let Some(entry) = self.values.get(&key) {
            return Admission::Duplicate(entry.clone());
        }
        if !self.ensure_room() {
            return Admission::AtCapacity;
        }
        let entry = Arc::new(DeliveryEntry::new(now));
        self.order.push_back(key.clone());
        self.values.insert(key, entry.clone());
        Admission::Owner(entry)
    }

    /// Drop completed entries past TTL. Insertion-ordered, so expired ones are a
    /// prefix; stop at the first live or in-flight one (never drop in-flight).
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

    /// Make room for one entry under the hard cap by evicting the oldest COMPLETED
    /// entry. In-flight entries are never evicted; false = all in-flight (reject).
    fn ensure_room(&mut self) -> bool {
        if self.values.len() < DEDUPE_CAPACITY {
            return true;
        }
        let victim = self
            .order
            .iter()
            .find(|k| self.values.get(*k).is_some_and(|e| e.is_complete()))
            .cloned();
        match victim {
            Some(key) => {
                self.values.remove(&key);
                self.order.retain(|k| k != &key);
                true
            }
            None => false,
        }
    }

    fn forget(&mut self, key: &str) {
        if self.values.remove(key).is_some() {
            self.order.retain(|k| k != key);
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

    /// Admit a delivery: own the dispatch, replay a duplicate, or reject when the
    /// window is saturated by in-flight leaders.
    pub fn acquire(&self, key: String, now: u64) -> Admission {
        self.store
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .acquire(key, now)
    }

    /// Drop a key so a later retry re-dispatches; used for not-delivered outcomes.
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
