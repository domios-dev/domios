//! Orchestrator-side view of remote (cross-mesh) workers. A worker spawned via
//! `agent spawn --mesh PEER` lives on another daemon, so it never appears in this
//! daemon's mux. It becomes visible here the moment its first `agent report`
//! arrives over the mesh: the inbound `/send` (kind=report) carries the peer,
//! the worker's foreign pane id, label, and model. We index those by the local
//! orchestrator/General pane they reported to, so the UI can render a "remote
//! pane" ghost node under its orchestrator — showing where it runs and its last
//! status — without owning a real PTY for it.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;

/// One remote worker, as last seen by the orchestrator it reports to.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
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

/// In-memory map: local orchestrator/General pane id → its remote workers.
/// Rebuilt from inbound reports; not persisted (a restarted orchestrator
/// repopulates as workers report again).
#[derive(Default)]
pub struct RemoteWorkerRegistry {
    by_orchestrator: Mutex<HashMap<String, Vec<RemoteWorker>>>,
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

/// Drop workers silent past `STALE_MS`, then prune now-empty orchestrator
/// buckets — bounds growth from closed orchestrators / churned panes.
fn evict_stale(map: &mut HashMap<String, Vec<RemoteWorker>>, now_ms: u64) {
    let cutoff = now_ms.saturating_sub(STALE_MS);
    map.retain(|_, list| {
        list.retain(|w| w.last_seen_ms >= cutoff);
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

impl RemoteWorkerRegistry {
    pub fn new() -> Self {
        Self::default()
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
        let entry = RemoteWorker {
            peer: peer.to_string(),
            remote_pane: remote_pane.to_string(),
            title: title.to_string(),
            model,
            status: status_from_body(body),
            last_message: body.chars().take(280).collect(),
            last_seen_ms: now_ms,
        };
        let mut map = self.by_orchestrator.lock();
        evict_stale(&mut map, now_ms);
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
        let mut map = self.by_orchestrator.lock();
        evict_stale(&mut map, now_ms);
        let list = map.entry(orchestrator_pane.to_string()).or_default();
        if list
            .iter()
            .any(|w| w.peer == peer && w.remote_pane == remote_pane)
        {
            return;
        }
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

    /// Drop an orchestrator's bucket when its pane closes.
    pub fn forget_orchestrator(&self, orchestrator_pane: &str) {
        self.by_orchestrator.lock().remove(orchestrator_pane);
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
    fn status_parsing_picks_lifecycle_token() {
        assert_eq!(status_from_body("STATUS: done | shipped"), "done");
        assert_eq!(status_from_body("status:blocked, need creds"), "blocked");
        assert_eq!(status_from_body("STATUS: partial"), "partial");
        assert_eq!(status_from_body("just working on it"), "working");
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
            "STATUS: done",
            20,
        );
        reg.record_report("orch", "eight", "w2", "Worker-2", None, "working", 15);

        let list = reg.for_orchestrator("orch");
        assert_eq!(list.len(), 2, "two distinct remote panes");
        // Newest activity first.
        assert_eq!(list[0].remote_pane, "w1");
        assert_eq!(
            list[0].status, "done",
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

        // A real report upserts the same (peer, pane) — no duplicate, richer status.
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
        assert_eq!(list.len(), 1, "same worker, no duplicate");
        assert_eq!(list[0].status, "done");

        // Re-spawn registration must NOT downgrade the reported `done` status.
        reg.record_spawn("orch", "eight", "w1", "Canvas", None, 300);
        let list = reg.for_orchestrator("orch");
        assert_eq!(list.len(), 1);
        assert_eq!(
            list[0].status, "done",
            "spawn re-registration must not clobber a real report"
        );
    }

    #[test]
    fn forget_orchestrator_drops_bucket() {
        let reg = RemoteWorkerRegistry::new();
        reg.record_report("orch", "eight", "w1", "W", None, "working", 1);
        reg.forget_orchestrator("orch");
        assert!(reg.for_orchestrator("orch").is_empty());
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
