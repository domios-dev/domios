use parking_lot::Mutex;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::LazyLock,
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) const COALESCE_WINDOW_MS: u64 = 10_000;
const MAX_ENTRIES: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RefusalLogKey {
    pub remote_ip: String,
    pub path: String,
    pub credential_kind: &'static str,
    pub reason: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RefusalLogDecision {
    Emit { suppressed: u64 },
    Suppress,
}

struct Entry {
    last_emitted_ms: u64,
    suppressed: u64,
}

pub(crate) struct RefusalLogCoalescer {
    window_ms: u64,
    max_entries: usize,
    entries: HashMap<RefusalLogKey, Entry>,
}

impl RefusalLogCoalescer {
    pub(crate) fn new(window_ms: u64, max_entries: usize) -> Self {
        Self {
            window_ms: window_ms.max(1),
            max_entries: max_entries.max(1),
            entries: HashMap::new(),
        }
    }

    pub(crate) fn observe(&mut self, key: RefusalLogKey, now_ms: u64) -> RefusalLogDecision {
        if let Some(entry) = self.entries.get_mut(&key) {
            if now_ms.saturating_sub(entry.last_emitted_ms) < self.window_ms {
                entry.suppressed = entry.suppressed.saturating_add(1);
                return RefusalLogDecision::Suppress;
            }
            let suppressed = entry.suppressed;
            entry.last_emitted_ms = now_ms;
            entry.suppressed = 0;
            return RefusalLogDecision::Emit { suppressed };
        }
        if self.entries.len() >= self.max_entries {
            self.entries
                .retain(|_, entry| now_ms.saturating_sub(entry.last_emitted_ms) < self.window_ms);
            if self.entries.len() >= self.max_entries {
                if let Some(oldest) = self
                    .entries
                    .iter()
                    .min_by_key(|(_, entry)| entry.last_emitted_ms)
                    .map(|(k, _)| k.clone())
                {
                    self.entries.remove(&oldest);
                }
            }
        }
        self.entries.insert(
            key,
            Entry {
                last_emitted_ms: now_ms,
                suppressed: 0,
            },
        );
        RefusalLogDecision::Emit { suppressed: 0 }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub(crate) fn suppressed_for(&self, key: &RefusalLogKey) -> u64 {
        self.entries
            .get(key)
            .map(|entry| entry.suppressed)
            .unwrap_or(0)
    }
}

static AUTH_401_COALESCER: LazyLock<Mutex<RefusalLogCoalescer>> =
    LazyLock::new(|| Mutex::new(RefusalLogCoalescer::new(COALESCE_WINDOW_MS, MAX_ENTRIES)));

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub(super) fn observe(
    remote_address: Option<SocketAddr>,
    path: &str,
    credential_kind: &'static str,
    reason: &'static str,
) -> RefusalLogDecision {
    let key = RefusalLogKey {
        remote_ip: remote_address
            .map(|address| address.ip().to_string())
            .unwrap_or_else(|| "unknown".into()),
        path: path.to_owned(),
        credential_kind,
        reason,
    };
    AUTH_401_COALESCER.lock().observe(key, now_ms())
}

#[cfg(test)]
pub(crate) fn snapshot_suppressed_for_test(
    remote_ip: &str,
    path: &str,
    credential_kind: &'static str,
    reason: &'static str,
) -> u64 {
    let key = RefusalLogKey {
        remote_ip: remote_ip.to_owned(),
        path: path.to_owned(),
        credential_kind,
        reason,
    };
    AUTH_401_COALESCER.lock().suppressed_for(&key)
}

#[cfg(test)]
pub(crate) fn reset_global_for_test() {
    AUTH_401_COALESCER.lock().entries.clear();
}

#[cfg(test)]
mod tests;
