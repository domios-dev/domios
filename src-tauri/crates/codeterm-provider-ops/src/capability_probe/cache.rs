use codeterm_plugin::model_catalog::BinaryFingerprint;
use codeterm_types::agent::capability_probe::{ProbeFailure, ProbeSnapshot};
use std::{collections::BTreeMap, path::PathBuf};
pub(super) type Key = (String, PathBuf);
struct Entry<T> {
    token: u64,
    binary: BinaryFingerprint,
    request: T,
    snapshot: ProbeSnapshot,
}
pub(super) struct ProbeCache<T> {
    entries: BTreeMap<Key, Entry<T>>,
    next: u64,
}
impl<T> Default for ProbeCache<T> {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
            next: 0,
        }
    }
}
pub(super) enum Start {
    Cached(ProbeSnapshot),
    Run(u64),
}
impl<T> ProbeCache<T> {
    pub(super) fn begin(
        &mut self,
        key: Key,
        binary: BinaryFingerprint,
        request: T,
        force: bool,
    ) -> Start {
        if !force {
            if let Some(entry) = self
                .entries
                .get(&key)
                .filter(|entry| entry.binary == binary)
            {
                return Start::Cached(entry.snapshot.clone());
            }
        }
        if self.entries.len() >= 64 && !self.entries.contains_key(&key) {
            return Start::Cached(ProbeSnapshot::Unavailable {
                reason: ProbeFailure::CacheLimit,
            });
        }
        self.next = self.next.wrapping_add(1);
        let token = self.next;
        self.entries.insert(
            key,
            Entry {
                token,
                binary,
                request,
                snapshot: ProbeSnapshot::Pending,
            },
        );
        Start::Run(token)
    }
    pub(super) fn finish(&mut self, key: &Key, token: u64, snapshot: ProbeSnapshot) -> bool {
        let Some(entry) = self
            .entries
            .get_mut(key)
            .filter(|entry| entry.token == token)
        else {
            return false;
        };
        entry.snapshot = snapshot;
        true
    }
    pub(super) fn invalidate(&mut self, provider: &str) -> Vec<T> {
        let keys: Vec<_> = self
            .entries
            .keys()
            .filter(|(id, _)| id == provider)
            .cloned()
            .collect();
        keys.into_iter()
            .filter_map(|key| self.entries.remove(&key).map(|entry| entry.request))
            .collect()
    }
}
