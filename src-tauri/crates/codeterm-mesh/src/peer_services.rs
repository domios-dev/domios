//! Last-known services per paired peer (#119/#127).
//!
//! The daemon-side source of truth for "what services does peer X have".
//! Refreshed on every successful fetch; retained (and served as stale) while
//! the peer is unreachable, so a peer restart no longer makes its services
//! vanish from the local view. Entries die on unpair, not on offline.

use std::collections::HashMap;

use codeterm_types::mesh::inventory::InventoryFreshness;
use serde::Serialize;

/// How long a cached fetch is considered fresh enough to skip the network.
/// The Mesh tab polls every 5s; this keeps one fetch per peer per window.
pub const FRESH_WINDOW_SECS: u64 = 10;

#[derive(Debug, Clone, Serialize)]
pub struct PeerServicesEntry {
    pub services: Vec<serde_json::Value>,
    pub fetched_at: u64,
}

#[derive(Default)]
pub struct PeerServicesCache {
    entries: HashMap<String, PeerServicesEntry>,
}

impl PeerServicesCache {
    pub fn refresh(&mut self, instance_id: &str, services: Vec<serde_json::Value>, now: u64) {
        self.entries.insert(
            instance_id.to_string(),
            PeerServicesEntry {
                services,
                fetched_at: now,
            },
        );
    }

    pub fn get(&self, instance_id: &str) -> Option<&PeerServicesEntry> {
        self.entries.get(instance_id)
    }

    pub fn is_fresh(&self, instance_id: &str, now: u64) -> bool {
        self.entries
            .get(instance_id)
            .map(|e| now.saturating_sub(e.fetched_at) < FRESH_WINDOW_SECS)
            .unwrap_or(false)
    }

    pub fn prune(&mut self, instance_id: &str) {
        self.entries.remove(instance_id);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// What the UI gets for one peer — always an honest envelope, never a silent
/// empty list: stale data is flagged stale, fetch errors are carried along.
#[derive(Debug, Clone, Serialize)]
pub struct PeerServicesView {
    pub services: Vec<serde_json::Value>,
    /// True when `services` is last-known data, not a fresh fetch.
    pub stale: bool,
    pub fetched_at: Option<u64>,
    pub peer_online: bool,
    pub error: Option<String>,
    pub freshness: InventoryFreshness,
    pub partial_error: Option<String>,
}

/// Result of one fetch attempt against the peer.
pub enum FetchOutcome {
    /// Fresh service list fetched now.
    Fetched(Vec<serde_json::Value>),
    /// Fetch attempted and failed (all addresses).
    Failed(String),
    /// Fetch skipped (peer flagged offline, or cache fresh).
    Skipped,
}

pub fn build_view(
    cached: Option<&PeerServicesEntry>,
    peer_online: bool,
    outcome: FetchOutcome,
    now: u64,
) -> PeerServicesView {
    match outcome {
        FetchOutcome::Fetched(services) => PeerServicesView {
            services,
            stale: false,
            fetched_at: Some(now),
            peer_online,
            error: None,
            freshness: InventoryFreshness::Fresh,
            partial_error: None,
        },
        FetchOutcome::Failed(err) => PeerServicesView {
            services: cached
                .map(|e| annotate_cached_services(e.services.clone(), peer_online, Some(&err)))
                .unwrap_or_default(),
            stale: true,
            fetched_at: cached.map(|e| e.fetched_at),
            peer_online,
            error: Some(err.clone()),
            freshness: if peer_online {
                InventoryFreshness::Stale
            } else {
                InventoryFreshness::Offline
            },
            partial_error: Some(err),
        },
        FetchOutcome::Skipped => PeerServicesView {
            services: cached
                .map(|e| {
                    if peer_online && now.saturating_sub(e.fetched_at) < FRESH_WINDOW_SECS {
                        e.services.clone()
                    } else {
                        annotate_cached_services(e.services.clone(), peer_online, None)
                    }
                })
                .unwrap_or_default(),
            stale: !cached
                .map(|e| now.saturating_sub(e.fetched_at) < FRESH_WINDOW_SECS)
                .unwrap_or(false),
            fetched_at: cached.map(|e| e.fetched_at),
            peer_online,
            error: None,
            freshness: if !peer_online {
                InventoryFreshness::Offline
            } else if cached
                .map(|e| now.saturating_sub(e.fetched_at) < FRESH_WINDOW_SECS)
                .unwrap_or(false)
            {
                InventoryFreshness::Fresh
            } else {
                InventoryFreshness::Stale
            },
            partial_error: None,
        },
    }
}

fn annotate_cached_services(
    mut services: Vec<serde_json::Value>,
    peer_online: bool,
    error: Option<&String>,
) -> Vec<serde_json::Value> {
    let freshness = if !peer_online { "offline" } else { "stale" };
    let reason = if error.is_some() {
        "partial_peer_failure"
    } else if peer_online {
        "stale"
    } else {
        "offline"
    };
    for service in &mut services {
        let Some(object) = service.as_object_mut() else {
            continue;
        };
        object.insert(
            "provenance".into(),
            serde_json::json!({
                "freshness": freshness,
                "offline": !peer_online,
                "partial_error": error,
            }),
        );
        let Some(capabilities) = object
            .get_mut("capabilities")
            .and_then(serde_json::Value::as_object_mut)
        else {
            continue;
        };
        for operation in [
            "update_schedule",
            "delete",
            "enable",
            "disable",
            "trigger",
            "cancel",
        ] {
            let Some(capability) = capabilities
                .get_mut(operation)
                .and_then(serde_json::Value::as_object_mut)
            else {
                continue;
            };
            if capability
                .get("allowed")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
            {
                capability.insert("allowed".into(), serde_json::Value::Bool(false));
                capability.insert(
                    "unavailable_reason".into(),
                    serde_json::Value::String(reason.into()),
                );
            }
        }
    }
    services
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svc(name: &str) -> serde_json::Value {
        serde_json::json!({"name": name})
    }

    #[test]
    fn refresh_replaces_wholesale() {
        let mut cache = PeerServicesCache::default();
        cache.refresh("p1", vec![svc("a"), svc("b")], 100);
        cache.refresh("p1", vec![svc("c")], 110);
        let entry = cache.get("p1").unwrap();
        assert_eq!(entry.services.len(), 1);
        assert_eq!(entry.fetched_at, 110);
    }

    #[test]
    fn freshness_window() {
        let mut cache = PeerServicesCache::default();
        cache.refresh("p1", vec![svc("a")], 100);
        assert!(cache.is_fresh("p1", 100 + FRESH_WINDOW_SECS - 1));
        assert!(!cache.is_fresh("p1", 100 + FRESH_WINDOW_SECS));
        assert!(!cache.is_fresh("unknown", 100));
    }

    #[test]
    fn prune_on_unpair_removes_entry() {
        let mut cache = PeerServicesCache::default();
        cache.refresh("p1", vec![svc("a")], 100);
        cache.prune("p1");
        assert!(cache.get("p1").is_none());
    }

    /// #119 regression: a failed fetch (peer restarting) must serve the
    /// last-known services flagged stale — not an empty list.
    #[test]
    fn failed_fetch_serves_cached_services_as_stale() {
        let mut cache = PeerServicesCache::default();
        cache.refresh("p1", vec![svc("a"), svc("b")], 100);
        let view = build_view(
            cache.get("p1"),
            false,
            FetchOutcome::Failed("all addresses failed".into()),
            200,
        );
        assert_eq!(view.services.len(), 2, "cached services vanished");
        assert!(view.stale);
        assert_eq!(view.fetched_at, Some(100));
        assert!(!view.peer_online);
        assert!(view.error.is_some());
    }

    /// #119: offline peer skipped fetch → cached list, stale flag.
    #[test]
    fn skipped_fetch_offline_peer_serves_cache_stale() {
        let mut cache = PeerServicesCache::default();
        cache.refresh("p1", vec![svc("a")], 100);
        let view = build_view(cache.get("p1"), false, FetchOutcome::Skipped, 500);
        assert_eq!(view.services.len(), 1);
        assert!(view.stale);
        assert!(view.error.is_none());
    }

    /// Fresh-window skip is NOT stale — it is just-fetched data.
    #[test]
    fn skipped_fetch_within_fresh_window_is_not_stale() {
        let mut cache = PeerServicesCache::default();
        cache.refresh("p1", vec![svc("a")], 100);
        let view = build_view(cache.get("p1"), true, FetchOutcome::Skipped, 105);
        assert!(!view.stale);
    }

    /// #127 regression: fetch failure with no cache must surface the error,
    /// not silently collapse to an empty success.
    #[test]
    fn failed_fetch_without_cache_carries_error() {
        let view = build_view(None, true, FetchOutcome::Failed("timeout".into()), 100);
        assert!(view.services.is_empty());
        assert!(view.stale);
        assert_eq!(view.error.as_deref(), Some("timeout"));
    }

    #[test]
    fn successful_fetch_is_fresh_view() {
        let view = build_view(None, true, FetchOutcome::Fetched(vec![svc("a")]), 100);
        assert!(!view.stale);
        assert_eq!(view.fetched_at, Some(100));
        assert!(view.error.is_none());
    }

    #[test]
    fn cached_capabilities_are_revoked_when_the_peer_is_offline() {
        let cached = serde_json::json!({
            "name": "cron",
            "provenance": {"freshness": "fresh", "offline": false},
            "capabilities": {
                "update_schedule": {"allowed": true},
                "delete": {"allowed": true}
            }
        });
        let mut cache = PeerServicesCache::default();
        cache.refresh("p1", vec![cached], 100);
        let view = build_view(cache.get("p1"), false, FetchOutcome::Skipped, 500);
        let resource = &view.services[0];
        assert_eq!(resource["provenance"]["freshness"], "offline");
        assert_eq!(
            resource["capabilities"]["update_schedule"]["allowed"],
            false
        );
        assert_eq!(
            resource["capabilities"]["update_schedule"]["unavailable_reason"],
            "offline"
        );
    }
}
