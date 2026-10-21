use std::collections::HashMap;

use codeterm_plugin::provider_cli::ProviderCliStatus;

use crate::agent::provider::ProviderAvailability;

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

/// No revision moves when a CLI is installed, so a memoized miss is served
/// only while its binary still resolves to nothing.
pub(super) fn misses_still_hold<'a>(
    unresolved: impl IntoIterator<Item = &'a str>,
    found: impl Fn(&str) -> bool,
) -> bool {
    unresolved.into_iter().all(|binary| !found(binary))
}

pub(super) fn binary_found(binary: &str) -> bool {
    matches!(
        codeterm_plugin::js_runtime::probe_bin(binary).resolution,
        codeterm_plugin::js_runtime::BinResolution::Found(_)
    )
}

pub(super) fn unresolved_cli(status: &ProviderCliStatus) -> Option<&str> {
    (status.required && status.availability != ProviderAvailability::Available)
        .then_some(status.binary.as_deref())
        .flatten()
}

pub(super) fn unresolved_listing(values: &[serde_json::Value]) -> impl Iterator<Item = &str> {
    values
        .iter()
        .filter(|value| value["availability"] != "available")
        .filter_map(|value| value["cli_status"]["binary"].as_str())
}

#[cfg(test)]
mod tests {
    use super::{misses_still_hold, unresolved_cli, unresolved_listing, RevisionCache};
    use crate::agent::provider::ProviderAvailability;

    #[test]
    fn a_memoized_miss_expires_once_its_binary_resolves() {
        let installed = |binary: &str| binary == "installed-fixture";
        assert!(misses_still_hold(["missing-fixture"], installed));
        assert!(!misses_still_hold(
            ["missing-fixture", "installed-fixture"],
            installed
        ));
        assert!(misses_still_hold([], |_: &str| true));
    }

    #[test]
    fn only_unresolved_binaries_are_revalidated() {
        let mut status = codeterm_plugin::provider_cli::no_cli();
        status.binary = Some("fixture-cli".into());
        assert_eq!(unresolved_cli(&status), None);
        status.required = true;
        status.availability = ProviderAvailability::Unavailable;
        assert_eq!(unresolved_cli(&status), Some("fixture-cli"));
        status.availability = ProviderAvailability::Available;
        assert_eq!(unresolved_cli(&status), None);

        let listing = vec![
            serde_json::json!({"availability": "available", "cli_status": {"binary": "found-cli"}}),
            serde_json::json!({"availability": "unavailable", "cli_status": {"binary": "missing-cli"}}),
            serde_json::json!({"availability": "unknown", "cli_status": {"binary": "unknown-cli"}}),
            serde_json::json!({"availability": "unavailable", "cli_status": null}),
        ];
        assert_eq!(
            unresolved_listing(&listing).collect::<Vec<_>>(),
            ["missing-cli", "unknown-cli"]
        );
    }

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
