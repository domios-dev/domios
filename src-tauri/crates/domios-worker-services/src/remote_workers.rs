//! Orchestrator-side view of remote (cross-mesh) workers. A worker spawned via
//! `agent spawn --mesh PEER` lives on another daemon, so it never appears in this
//! daemon's mux. It becomes visible here the moment its first `agent report`
//! arrives over the mesh: the inbound `/send` (kind=report) carries the peer,
//! the worker's foreign tab id, label, and model. We index those by the local
//! orchestrator/General tab they reported to, so the UI can render a "remote
//! Tab" ghost node under its orchestrator — showing where it runs and its last
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
    /// The worker's tab id on that peer (its `CODETERM_TAB_ID`).
    pub remote_tab: String,
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

/// Map: local orchestrator/General tab id → its remote workers. Mutated by
/// inbound reports + spawn registration, and (when `path` is set) persisted to
/// disk so recent ghosts survive an origin restart without waiting for a new
/// report. `path` is None in tests → pure in-memory, no I/O.
#[derive(Default)]
pub struct RemoteWorkerRegistry {
    by_orchestrator: Mutex<HashMap<String, Vec<RemoteWorker>>>,
    path: Option<PathBuf>,
}

/// Drop workers silent for longer than this; their orchestrator bucket is
/// removed once empty. Bounds growth from closed orchestrators / churned tabs.
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
    /// Missing/corrupt records are quarantined in memory and left unchanged on disk.
    pub fn load(path: PathBuf) -> Self {
        let mut map = load_worker_records(&path);
        evict_inactive(&mut map, now_ms_epoch());
        Self {
            by_orchestrator: Mutex::new(map),
            path: Some(path),
        }
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
    /// (peer, remote_tab) under the orchestrator tab it reported to.
    #[allow(clippy::too_many_arguments)]
    pub fn record_report(
        &self,
        orchestrator_tab: &str,
        peer: &str,
        remote_tab: &str,
        title: &str,
        model: Option<String>,
        body: &str,
        now_ms: u64,
    ) {
        let status = status_from_body(body);
        if status == "done" {
            self.forget_by_remote_tab(remote_tab, Some(peer), Some(orchestrator_tab));
            return;
        }
        let entry = RemoteWorker {
            peer: peer.to_string(),
            remote_tab: remote_tab.to_string(),
            title: title.to_string(),
            model,
            status,
            last_message: body.chars().take(280).collect(),
            last_seen_ms: now_ms,
        };
        {
            let mut map = self.by_orchestrator.lock();
            evict_inactive(&mut map, now_ms);
            let list = map.entry(orchestrator_tab.to_string()).or_default();
            if let Some(slot) = list
                .iter_mut()
                .find(|w| w.peer == entry.peer && w.remote_tab == entry.remote_tab)
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
    /// by (peer, remote_tab) under the orchestrator/report-target tab. Status
    /// is `working` (just launched). Insert-only: if a real report already
    /// landed for this worker, that richer entry is kept untouched — spawn
    /// registration never downgrades a reported status back to `working`.
    pub fn record_spawn(
        &self,
        orchestrator_tab: &str,
        peer: &str,
        remote_tab: &str,
        title: &str,
        model: Option<String>,
        now_ms: u64,
    ) {
        {
            let mut map = self.by_orchestrator.lock();
            evict_inactive(&mut map, now_ms);
            let list = map.entry(orchestrator_tab.to_string()).or_default();
            if !list
                .iter()
                .any(|w| w.peer == peer && w.remote_tab == remote_tab)
            {
                list.push(RemoteWorker {
                    peer: peer.to_string(),
                    remote_tab: remote_tab.to_string(),
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

    /// Drop an orchestrator's bucket when its tab closes.
    pub fn forget_orchestrator(&self, orchestrator_tab: &str) {
        self.by_orchestrator.lock().remove(orchestrator_tab);
        self.persist();
    }

    /// Drop a remote-worker ghost by its foreign tab id. Optional `peer` and
    /// `orchestrator_tab` narrow the match. Returns orchestrator tab ids whose
    /// buckets changed (for `remote-workers-changed` emits). Empty ⇒ no match.
    pub fn forget_by_remote_tab(
        &self,
        remote_tab: &str,
        peer: Option<&str>,
        orchestrator_tab: Option<&str>,
    ) -> Vec<String> {
        let mut touched = Vec::new();
        {
            let mut map = self.by_orchestrator.lock();
            let keys: Vec<String> = match orchestrator_tab {
                Some(orch) => vec![orch.to_string()],
                None => map.keys().cloned().collect(),
            };
            for key in keys {
                let Some(list) = map.get_mut(&key) else {
                    continue;
                };
                let before = list.len();
                list.retain(|w| {
                    if w.remote_tab != remote_tab {
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

    /// Remote workers reporting to one orchestrator tab, newest activity first.
    pub fn for_orchestrator(&self, orchestrator_tab: &str) -> Vec<RemoteWorker> {
        let mut list = self
            .by_orchestrator
            .lock()
            .get(orchestrator_tab)
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

fn load_worker_records(path: &std::path::Path) -> HashMap<String, Vec<RemoteWorker>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return HashMap::new(),
        Err(error) => {
            log::error!(
                "[remote-workers] startup read failed for {}: {}",
                path.display(),
                error
            );
            return HashMap::new();
        }
    };
    let root: serde_json::Value = match serde_json::from_str(&text) {
        Ok(root) => root,
        Err(error) => {
            log::error!(
                "[remote-workers] quarantined unreadable store {}: {}; source left unchanged",
                path.display(),
                error
            );
            return HashMap::new();
        }
    };
    let Some(buckets) = root.as_object() else {
        log::error!(
            "[remote-workers] store {} is not an object; source left unchanged",
            path.display()
        );
        return HashMap::new();
    };
    let mut loaded = HashMap::new();
    for (orchestrator_tab, records) in buckets {
        let Some(records) = records.as_array() else {
            log::error!(
                "[remote-workers] quarantined bucket {} in {}: expected array; source left unchanged",
                orchestrator_tab,
                path.display()
            );
            continue;
        };
        let workers: Vec<RemoteWorker> = records
            .iter()
            .enumerate()
            .filter_map(|(index, record)| {
                parse_worker_record(path, orchestrator_tab, index, record.clone())
            })
            .collect();
        if !workers.is_empty() {
            loaded.insert(orchestrator_tab.clone(), workers);
        }
    }
    loaded
}

fn parse_worker_record(
    path: &std::path::Path,
    orchestrator_tab: &str,
    index: usize,
    mut record: serde_json::Value,
) -> Option<RemoteWorker> {
    let normalized = match normalize_worker_record(&mut record) {
        Ok(normalized) => normalized,
        Err(error) => {
            log::error!(
                "[remote-workers] quarantined record {}[{}] in {}: {}; source left unchanged",
                orchestrator_tab,
                index,
                path.display(),
                error
            );
            return None;
        }
    };
    match serde_json::from_value(record) {
        Ok(worker) => {
            if normalized {
                log::warn!(
                    "[remote-workers] normalized remote_pane in record {}[{}] from {}; source left unchanged",
                    orchestrator_tab,
                    index,
                    path.display()
                );
            }
            Some(worker)
        }
        Err(error) => {
            log::error!(
                "[remote-workers] quarantined record {}[{}] in {}: {}; source left unchanged",
                orchestrator_tab,
                index,
                path.display(),
                error
            );
            None
        }
    }
}

fn normalize_worker_record(record: &mut serde_json::Value) -> Result<bool, String> {
    let Some(object) = record.as_object_mut() else {
        return Err("record is not an object".into());
    };
    let Some(legacy_value) = object.get("remote_pane").cloned() else {
        return Ok(false);
    };
    if let Some(current_value) = object.get("remote_tab") {
        if current_value != &legacy_value {
            return Err("ambiguous fields remote_pane and remote_tab".into());
        }
    } else {
        object.insert("remote_tab".into(), legacy_value);
    }
    object.remove("remote_pane");
    Ok(true)
}

pub type RemoteWorkerRegistryHandle = Arc<RemoteWorkerRegistry>;

#[cfg(test)]
mod tests;
