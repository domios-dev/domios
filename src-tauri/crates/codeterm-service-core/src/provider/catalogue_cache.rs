use std::collections::HashMap;

pub(super) struct RevisionCache<K, V> {
    entries: HashMap<String, (K, V)>,
}

impl<K, V> Default for RevisionCache<K, V> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }
}

impl<K: Eq, V: Clone> RevisionCache<K, V> {
    pub fn get(&self, key: &str, revision: &K) -> Option<V> {
        self.entries
            .get(key)
            .filter(|(stored, _)| stored == revision)
            .map(|(_, value)| value.clone())
    }

    pub fn insert(&mut self, key: String, revision: K, value: V) {
        self.entries.insert(key, (revision, value));
    }
}

#[cfg(test)]
mod tests {
    use super::RevisionCache;

    #[test]
    fn repeated_listings_hit_and_provider_version_or_updater_changes_miss() {
        let mut cache = RevisionCache::default();
        let initial = (1, 2, 3);
        assert_eq!(cache.get("provider", &initial), None);
        cache.insert("provider".into(), initial, "cached-status");
        for _ in 0..100 {
            assert_eq!(cache.get("provider", &initial), Some("cached-status"));
        }
        for revision in [(2, 2, 3), (1, 3, 3), (1, 2, 4)] {
            assert_eq!(cache.get("provider", &revision), None);
        }
        assert_eq!(cache.get("other-provider", &initial), None);
        cache.insert("provider".into(), (1, 3, 3), "new-version");
        assert_eq!(cache.get("provider", &(1, 3, 3)), Some("new-version"));
    }
}
