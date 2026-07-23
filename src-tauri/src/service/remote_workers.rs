//! Orchestrator-side view of remote (cross-mesh) workers. A worker spawned via
//! `agent spawn --mesh PEER` lives on another daemon, so it never appears in this
//! daemon's mux. It becomes visible here the moment its first `agent report`
//! arrives over the mesh: the inbound `/send` (kind=report) carries the peer,
//! the worker's foreign pane id, label, and model. We index those by the local
//! orchestrator/General pane they reported to, so the UI can render a "remote
//! pane" ghost node under its orchestrator — showing where it runs and its last
//! status — without owning a real PTY for it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// One remote worker, as last seen by the orchestrator it reports to.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteWorker {
    /// Paired peer (instance name) the worker runs on.
    pub peer: String,
    /// The worker's pane id on that peer (its `CODETERM_PANE_ID`).
    pub remote_pane: String,
    /// Display label (worker title), as reported.
    pub title: String,
    /// Model family, when the report carried one.
    pub model: Option<String>,
    /// Last reported lifecycle status: `working` | `done` | `blocked` | `partial`.
    pub status: String,
    /// Last report body (trimmed for display).
    pub last_message: String,
    /// Wall-clock of the last report (ms since epoch), stamped by the caller.
    pub last_seen_ms: u64,
}

/// Map: local orchestrator/General pane id → its remote workers. Mutated by
/// inbound reports + spawn registration, and (when `path` is set) persisted to
/// disk so recent ghosts survive an origin restart without waiting for a new
/// report. `path` is None in tests → pure in-memory, no I/O.
#[derive(Default)]
pub struct RemoteWorkerRegistry {
    by_orchestrator: Mutex<HashMap<String, Vec<RemoteWorker>>>,
    path: Option<PathBuf>,
}

/// Drop workers silent for longer than this; their orchestrator bucket is
/// removed once empty. Bounds growth from closed orchestrators / churned panes.
const STALE_MS: u64 = 24 * 60 * 60 * 1000;
/// Hard cap per orchestrator (oldest activity evicted first).
const MAX_PER_ORCHESTRATOR: usize = 64;

/// Derive a coarse status from a report body. Worker reports conventionally lead
/// with `STATUS: done|blocked|partial`; absent that, the worker is still working.
pub fn status_from_body(body: &str) -> String {
    let lower = body.to_ascii_lowercase();
    if let Some(idx) = lower.find("status:") {
        let rest = lower[idx + "status:".len()..].trim_start();
        for s in ["done", "blocked", "partial"] {
            if rest.starts_with(s) {
                return s.to_string();
            }
        }
    }
    "working".to_string()
}

/// Drop completed or silent workers, then prune empty orchestrator buckets.
fn evict_inactive(map: &mut HashMap<String, Vec<RemoteWorker>>, now_ms: u64) {
    let cutoff = now_ms.saturating_sub(STALE_MS);
    map.retain(|_, list| {
        list.retain(|w| w.status != "done" && w.last_seen_ms >= cutoff);
        !list.is_empty()
    });
}

/// Enforce the per-orchestrator cap, evicting oldest activity first.
fn cap_bucket(list: &mut Vec<RemoteWorker>) {
    if list.len() > MAX_PER_ORCHESTRATOR {
        list.sort_by(|a, b| b.last_seen_ms.cmp(&a.last_seen_ms));
        list.truncate(MAX_PER_ORCHESTRATOR);
    }
}

fn now_ms_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

impl RemoteWorkerRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct a registry backed by `path`, keeping only active recent workers.
    /// Missing/corrupt file → empty (best-effort).
    pub fn load(path: PathBuf) -> Self {
        let persisted = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<HashMap<String, Vec<RemoteWorker>>>(&s).ok());
        let mut map = persisted.clone().unwrap_or_default();
        evict_inactive(&mut map, now_ms_epoch());
        let registry = Self {
            by_orchestrator: Mutex::new(map),
            path: Some(path),
        };
        if persisted.is_some() {
            registry.persist();
        }
        registry
    }

    /// Atomically write the current map to the durable path (no-op without
    /// one). Best-effort: a write hiccup must never fail a report.
    fn persist(&self) {
        let Some(path) = self.path.as_ref() else {
            return;
        };
        let snapshot = self.by_orchestrator.lock().clone();
        let Ok(json) = serde_json::to_string(&snapshot) else {
            return;
        };
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }

    /// Upsert a remote worker from an inbound mesh report. Keyed by
    /// (peer, remote_pane) under the orchestrator pane it reported to.
    #[allow(clippy::too_many_arguments)]
    pub fn record_report(
        &self,
        orchestrator_pane: &str,
        peer: &str,
        remote_pane: &str,
        title: &str,
        model: Option<String>,
        body: &str,
        now_ms: u64,
    ) {
        let status = status_from_body(body);
        if status == "done" {
            self.forget_by_remote_pane(remote_pane, Some(peer), Some(orchestrator_pane));
            return;
        }
        let entry = RemoteWorker {
            peer: peer.to_string(),
            remote_pane: remote_pane.to_string(),
            title: title.to_string(),
            model,
            status,
            last_message: body.chars().take(280).collect(),
            last_seen_ms: now_ms,
        };
        {
            let mut map = self.by_orchestrator.lock();
            evict_inactive(&mut map, now_ms);
            let list = map.entry(orchestrator_pane.to_string()).or_default();
            if let Some(slot) = list
                .iter_mut()
                .find(|w| w.peer == entry.peer && w.remote_pane == entry.remote_pane)
            {
                *slot = entry;
            } else {
                list.push(entry);
            }
            cap_bucket(list);
        }
        self.persist();
    }

    /// Register a remote worker at spawn time, before its first report. Keyed
    /// by (peer, remote_pane) under the orchestrator/report-target pane. Status
    /// is `working` (just launched). Insert-only: if a real report already
    /// landed for this worker, that richer entry is kept untouched — spawn
    /// registration never downgrades a reported status back to `working`.
    pub fn record_spawn(
        &self,
        orchestrator_pane: &str,
        peer: &str,
        remote_pane: &str,
        title: &str,
        model: Option<String>,
        now_ms: u64,
    ) {
        {
            let mut map = self.by_orchestrator.lock();
            evict_inactive(&mut map, now_ms);
            let list = map.entry(orchestrator_pane.to_string()).or_default();
            if !list
                .iter()
                .any(|w| w.peer == peer && w.remote_pane == remote_pane)
            {
                list.push(RemoteWorker {
                    peer: peer.to_string(),
                    remote_pane: remote_pane.to_string(),
                    title: title.to_string(),
                    model,
                    status: "working".to_string(),
                    last_message: "spawned via mesh".to_string(),
                    last_seen_ms: now_ms,
                });
                cap_bucket(list);
            }
        }
        self.persist();
    }

    /// Drop an orchestrator's bucket when its pane closes.
    pub fn forget_orchestrator(&self, orchestrator_pane: &str) {
        self.by_orchestrator.lock().remove(orchestrator_pane);
        self.persist();
    }

    /// Drop a remote-worker ghost by its foreign pane id. Optional `peer` and
    /// `orchestrator_pane` narrow the match. Returns orchestrator pane ids whose
    /// buckets changed (for `remote-workers-changed` emits). Empty ⇒ no match.
    pub fn forget_by_remote_pane(
        &self,
        remote_pane: &str,
        peer: Option<&str>,
        orchestrator_pane: Option<&str>,
    ) -> Vec<String> {
        let mut touched = Vec::new();
        {
            let mut map = self.by_orchestrator.lock();
            let keys: Vec<String> = match orchestrator_pane {
                Some(orch) => vec![orch.to_string()],
                None => map.keys().cloned().collect(),
            };
            for key in keys {
                let Some(list) = map.get_mut(&key) else {
                    continue;
                };
                let before = list.len();
                list.retain(|w| {
                    if w.remote_pane != remote_pane {
                        return true;
                    }
                    if let Some(p) = peer {
                        return w.peer != p;
                    }
                    false
                });
                if list.len() != before {
                    touched.push(key.clone());
                }
                if list.is_empty() {
                    map.remove(&key);
                }
            }
        }
        if !touched.is_empty() {
            self.persist();
        }
        touched
    }

    /// Remote workers reporting to one orchestrator pane, newest activity first.
    pub fn for_orchestrator(&self, orchestrator_pane: &str) -> Vec<RemoteWorker> {
        let mut list = self
            .by_orchestrator
            .lock()
            .get(orchestrator_pane)
            .cloned()
            .unwrap_or_default();
        list.sort_by(|a, b| b.last_seen_ms.cmp(&a.last_seen_ms));
        list
    }

    /// Full map for hydration. Empty orchestrators are omitted.
    pub fn snapshot(&self) -> HashMap<String, Vec<RemoteWorker>> {
        self.by_orchestrator.lock().clone()
    }
}

pub type RemoteWorkerRegistryHandle = Arc<RemoteWorkerRegistry>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_and_rehydrates_active_workers_across_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("remote_workers.json");
        let now = now_ms_epoch();
        {
            let reg = RemoteWorkerRegistry::load(path.clone());
            reg.record_spawn("orch", "eight", "w1", "Canvas", Some("opus".into()), now);
            reg.record_report(
                "orch",
                "eight",
                "w2",
                "Renderer",
                None,
                "STATUS: working",
                now,
            );
        } // drop — simulates daemon shutdown
        assert!(path.exists(), "registry persisted to disk");

        // Fresh registry from the same path = post-restart rehydrate.
        let reloaded = RemoteWorkerRegistry::load(path);
        let list = reloaded.for_orchestrator("orch");
        assert_eq!(
            list.len(),
            2,
            "active remote workers survive restart without a new report"
        );
        let canvas = list.iter().find(|w| w.remote_pane == "w1").unwrap();
        assert_eq!(canvas.peer, "eight");
        assert_eq!(canvas.status, "working");
        assert_eq!(canvas.model.as_deref(), Some("opus"));
        let renderer = list.iter().find(|w| w.remote_pane == "w2").unwrap();
        assert_eq!(renderer.status, "working");
    }

    #[test]
    fn load_drops_stale_entries_so_restart_restores_only_recent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("remote_workers.json");
        let now = now_ms_epoch();
        {
            let reg = RemoteWorkerRegistry::load(path.clone());
            // One fresh, one already older than STALE_MS.
            reg.record_report("orch", "eight", "fresh", "F", None, "working", now);
            reg.record_report(
                "orch",
                "eight",
                "old",
                "O",
                None,
                "working",
                now.saturating_sub(STALE_MS + 1),
            );
        }
        let reloaded = RemoteWorkerRegistry::load(path);
        let list = reloaded.for_orchestrator("orch");
        assert_eq!(list.len(), 1, "stale ghost evicted on rehydrate");
        assert_eq!(list[0].remote_pane, "fresh");
    }

    #[test]
    fn load_removes_completed_entries_from_persisted_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("remote_workers.json");
        let completed = RemoteWorker {
            peer: "eight".into(),
            remote_pane: "done".into(),
            title: "Done".into(),
            model: None,
            status: "done".into(),
            last_message: "STATUS: done".into(),
            last_seen_ms: now_ms_epoch(),
        };
        std::fs::write(
            &path,
            serde_json::to_string(&HashMap::from([("orch".to_string(), vec![completed])])).unwrap(),
        )
        .unwrap();

        let registry = RemoteWorkerRegistry::load(path.clone());
        assert!(registry.for_orchestrator("orch").is_empty());
        let persisted: HashMap<String, Vec<RemoteWorker>> =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert!(persisted.is_empty());
    }

    #[test]
    fn in_memory_registry_writes_nothing() {
        // new() has no path → pure in-memory, no disk I/O.
        let reg = RemoteWorkerRegistry::new();
        reg.record_spawn("orch", "eight", "w1", "W", None, now_ms_epoch());
        assert_eq!(reg.for_orchestrator("orch").len(), 1);
    }

    #[test]
    fn status_parsing_picks_lifecycle_token() {
        assert_eq!(status_from_body("STATUS: done | shipped"), "done");
        assert_eq!(status_from_body("status:blocked, need creds"), "blocked");
        assert_eq!(status_from_body("STATUS: partial"), "partial");
        assert_eq!(status_from_body("just working on it"), "working");
    }

    #[test]
    fn completed_report_removes_worker_from_active_registry() {
        let reg = RemoteWorkerRegistry::new();
        reg.record_spawn("orch", "eight", "w1", "Renderer", None, 100);
        reg.record_report("orch", "eight", "w1", "Renderer", None, "STATUS: done", 101);
        assert!(reg.for_orchestrator("orch").is_empty());
    }

    #[test]
    fn blocked_and_partial_reports_remain_actionable() {
        let reg = RemoteWorkerRegistry::new();
        reg.record_report(
            "orch",
            "eight",
            "blocked",
            "Blocked",
            None,
            "STATUS: blocked",
            100,
        );
        reg.record_report(
            "orch",
            "eight",
            "partial",
            "Partial",
            None,
            "STATUS: partial",
            101,
        );
        let workers = reg.for_orchestrator("orch");
        assert_eq!(workers.len(), 2);
        assert!(workers.iter().any(|worker| worker.status == "blocked"));
        assert!(workers.iter().any(|worker| worker.status == "partial"));
    }

    #[test]
    fn upsert_replaces_same_worker_and_keeps_distinct() {
        let reg = RemoteWorkerRegistry::new();
        reg.record_report(
            "orch",
            "eight",
            "w1",
            "Worker-1",
            None,
            "STATUS: working",
            10,
        );
        reg.record_report(
            "orch",
            "eight",
            "w1",
            "Worker-1",
            Some("opus".into()),
            "STATUS: partial",
            20,
        );
        reg.record_report("orch", "eight", "w2", "Worker-2", None, "working", 15);

        let list = reg.for_orchestrator("orch");
        assert_eq!(list.len(), 2, "two distinct remote panes");
        // Newest activity first.
        assert_eq!(list[0].remote_pane, "w1");
        assert_eq!(
            list[0].status, "partial",
            "same worker upserts, not duplicates"
        );
        assert_eq!(list[0].model.as_deref(), Some("opus"));
        assert_eq!(list[1].remote_pane, "w2");
    }

    #[test]
    fn stale_entries_and_empty_buckets_are_evicted() {
        let reg = RemoteWorkerRegistry::new();
        // Old worker under orch-a; a fresh report elsewhere triggers the sweep.
        reg.record_report("orch-a", "eight", "w1", "W", None, "working", 1);
        let fresh = super::STALE_MS + 1_000;
        reg.record_report("orch-b", "eight", "w2", "W", None, "working", fresh);
        assert!(
            reg.for_orchestrator("orch-a").is_empty(),
            "stale worker + its empty bucket must be evicted"
        );
        assert_eq!(reg.for_orchestrator("orch-b").len(), 1);
    }

    #[test]
    fn record_spawn_inserts_pending_worker_then_report_upgrades_without_clobber() {
        let reg = RemoteWorkerRegistry::new();
        // Spawn registers a pending worker before any report.
        reg.record_spawn("orch", "eight", "w1", "Canvas", Some("opus".into()), 100);
        let list = reg.for_orchestrator("orch");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].remote_pane, "w1");
        assert_eq!(list[0].status, "working");
        assert_eq!(list[0].model.as_deref(), Some("opus"));

        // A terminal report removes the completed worker from the active registry.
        reg.record_report(
            "orch",
            "eight",
            "w1",
            "Canvas",
            Some("opus".into()),
            "STATUS: done",
            200,
        );
        let list = reg.for_orchestrator("orch");
        assert!(
            list.is_empty(),
            "completed workers leave the active registry"
        );

        // A later spawn represents a fresh worker lifecycle for the pane id.
        reg.record_spawn("orch", "eight", "w1", "Canvas", None, 300);
        let list = reg.for_orchestrator("orch");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].status, "working");
    }

    #[test]
    fn forget_orchestrator_drops_bucket() {
        let reg = RemoteWorkerRegistry::new();
        reg.record_report("orch", "eight", "w1", "W", None, "working", 1);
        reg.forget_orchestrator("orch");
        assert!(reg.for_orchestrator("orch").is_empty());
    }

    #[test]
    fn forget_by_remote_pane_removes_ghost_and_empty_bucket() {
        let reg = RemoteWorkerRegistry::new();
        reg.record_spawn("orch", "rollacode-bot", "6d2e36b4", "Main", None, 100);
        reg.record_spawn("orch", "rollacode-bot", "aaaa1111", "Other", None, 101);

        let touched = reg.forget_by_remote_pane("6d2e36b4", None, None);
        assert_eq!(touched, vec!["orch".to_string()]);
        let list = reg.for_orchestrator("orch");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].remote_pane, "aaaa1111");
    }

    #[test]
    fn forget_by_remote_pane_respects_peer_filter() {
        let reg = RemoteWorkerRegistry::new();
        reg.record_spawn("orch", "peer-a", "w1", "A", None, 1);
        reg.record_spawn("orch", "peer-b", "w1", "B", None, 2);

        let touched = reg.forget_by_remote_pane("w1", Some("peer-a"), None);
        assert_eq!(touched, vec!["orch".to_string()]);
        let list = reg.for_orchestrator("orch");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].peer, "peer-b");
    }

    #[test]
    fn forget_by_remote_pane_scopes_to_one_orchestrator() {
        let reg = RemoteWorkerRegistry::new();
        reg.record_spawn("orch-a", "peer", "w1", "W", None, 1);
        reg.record_spawn("orch-b", "peer", "w1", "W", None, 2);

        let touched = reg.forget_by_remote_pane("w1", None, Some("orch-a"));
        assert_eq!(touched, vec!["orch-a".to_string()]);
        assert!(reg.for_orchestrator("orch-a").is_empty());
        assert_eq!(reg.for_orchestrator("orch-b").len(), 1);
    }

    #[test]
    fn forget_by_remote_pane_no_match_returns_empty() {
        let reg = RemoteWorkerRegistry::new();
        reg.record_spawn("orch", "peer", "w1", "W", None, 1);
        assert!(reg.forget_by_remote_pane("missing", None, None).is_empty());
        assert_eq!(reg.for_orchestrator("orch").len(), 1);
    }

    #[test]
    fn distinct_orchestrators_are_isolated() {
        let reg = RemoteWorkerRegistry::new();
        reg.record_report("orch-a", "eight", "w1", "W", None, "working", 1);
        reg.record_report("orch-b", "eight", "w2", "W", None, "working", 1);
        assert_eq!(reg.for_orchestrator("orch-a").len(), 1);
        assert_eq!(reg.for_orchestrator("orch-b").len(), 1);
        assert!(reg.for_orchestrator("orch-c").is_empty());
    }
}
