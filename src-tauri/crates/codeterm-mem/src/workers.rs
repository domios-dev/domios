use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn build_worker_task(intent: &str, worker_id: &str, task: &str) -> String {
    crate::prompt_text::WORKER_TASK_PREAMBLE
        .replace("{intent}", intent)
        .replace("{worker_id}", worker_id)
        .replace("{task}", task)
}

#[derive(Clone, Debug)]
pub struct MemWorkerSpawnParams {
    pub intent: String,
    pub task_prompt: String,
    pub spawned_by: String,
    pub agent_provider: String,
    pub model: String,
}

const COLOR_PALETTE: [&str; 12] = [
    "hsl(  0, 70%, 60%)",
    "hsl( 30, 75%, 58%)",
    "hsl( 60, 70%, 55%)",
    "hsl( 90, 65%, 55%)",
    "hsl(120, 60%, 55%)",
    "hsl(150, 60%, 55%)",
    "hsl(180, 60%, 55%)",
    "hsl(210, 70%, 60%)",
    "hsl(240, 70%, 65%)",
    "hsl(270, 65%, 65%)",
    "hsl(300, 65%, 60%)",
    "hsl(330, 70%, 60%)",
];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkerStatus {
    Queued,
    Starting,
    Reading,
    Analyzing,
    Writing,
    Done,
    Error,
    Orphaned,
}

impl WorkerStatus {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "queued" => Some(Self::Queued),
            "starting" => Some(Self::Starting),
            "reading" => Some(Self::Reading),
            "analyzing" => Some(Self::Analyzing),
            "writing" => Some(Self::Writing),
            "done" => Some(Self::Done),
            "error" => Some(Self::Error),
            "orphaned" => Some(Self::Orphaned),
            _ => None,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Done | Self::Error | Self::Orphaned)
    }
}

/// Intent-string prefix for any dreaming-phase agent worker (currently only
/// `"dreaming:rem"`). Spawn site sets it; timeout-selection in service::spawn
/// matches via [`is_dreaming_intent`]. Single source of truth.
pub const DREAMING_INTENT_PREFIX: &str = "dreaming:";

/// Returns true if the worker intent identifies a dreaming-phase agent (uses
/// `dreaming_timeout_seconds` instead of the default `worker_timeout_seconds`).
#[inline]
pub fn is_dreaming_intent(intent: &str) -> bool {
    intent.starts_with(DREAMING_INTENT_PREFIX)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphWorkerInfo {
    pub worker_id: String,
    pub tab_id: String,
    pub intent: String,
    pub spawned_by: String,
    pub spawned_at: u64,
    pub status: WorkerStatus,
    pub progress_text: Option<String>,
    pub touched_nodes: HashSet<String>,
    pub color: String,
    pub report: Option<String>,
    pub done_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    // session_id + cwd captured before the hidden tab closes so the read-only chat-ui can resolve and render this run's jsonl after the fact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    // inverse seam — recorded from `host.agent.spawn(commands)` (C2 MINOR-2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<String>,
}

pub struct GraphWorkerRegistry {
    inner: RwLock<HashMap<String, GraphWorkerInfo>>,
    persist_path: PathBuf,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn new_worker_id() -> String {
    let uuid = uuid::Uuid::new_v4();
    let bytes = uuid.as_bytes();
    format!(
        "gw_{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3]
    )
}

impl GraphWorkerRegistry {
    pub fn load(persist_path: PathBuf) -> Arc<Self> {
        let mut inner: HashMap<String, GraphWorkerInfo> = HashMap::new();
        if persist_path.exists() {
            match std::fs::read_to_string(&persist_path) {
                Ok(text) => load_worker_records(&persist_path, &text, &mut inner),
                Err(error) => log::error!(
                    "[graph-workers] startup read failed for {}: {}",
                    persist_path.display(),
                    error
                ),
            }
        }
        Arc::new(Self {
            inner: RwLock::new(inner),
            persist_path,
        })
    }

    fn persist(&self) {
        const MAX_HISTORY: usize = 200;
        {
            let mut map = self.inner.write();
            let total = map.len();
            if total > MAX_HISTORY {
                let mut done_workers: Vec<(String, u64, Option<String>)> = map
                    .values()
                    .filter(|w| w.status.is_terminal())
                    .map(|w| (w.worker_id.clone(), w.spawned_at, w.log_path.clone()))
                    .collect();
                done_workers.sort_by_key(|(_, ts, _)| *ts);
                let to_remove = total - MAX_HISTORY;
                for (id, _, log_path) in done_workers.into_iter().take(to_remove) {
                    map.remove(&id);
                    if let Some(path) = log_path {
                        let _ = std::fs::remove_file(&path);
                    }
                }
            }
        }

        let items: Vec<GraphWorkerInfo> = self.inner.read().values().cloned().collect();
        if let Some(parent) = self.persist_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match serde_json::to_string_pretty(&items) {
            Ok(text) => {
                if let Err(e) = std::fs::write(&self.persist_path, text) {
                    log::warn!("[graph-workers] persist write: {}", e);
                }
            }
            Err(e) => log::warn!("[graph-workers] persist serialize: {}", e),
        }
    }

    pub fn pick_color(&self) -> String {
        let active_count = self
            .inner
            .read()
            .values()
            .filter(|w| !w.status.is_terminal())
            .count();
        COLOR_PALETTE[active_count % COLOR_PALETTE.len()].to_string()
    }

    pub fn register(&self, worker: GraphWorkerInfo) {
        self.inner.write().insert(worker.worker_id.clone(), worker);
        self.persist();
    }

    pub fn list(&self) -> Vec<GraphWorkerInfo> {
        self.inner.read().values().cloned().collect()
    }

    pub fn get(&self, worker_id: &str) -> Option<GraphWorkerInfo> {
        self.inner.read().get(worker_id).cloned()
    }

    pub fn active_by_intent(&self, intent: &str) -> Option<String> {
        self.inner
            .read()
            .values()
            .find(|w| w.intent == intent && !w.status.is_terminal())
            .map(|w| w.worker_id.clone())
    }

    pub fn update_status(&self, worker_id: &str, status: WorkerStatus) -> Result<(), String> {
        {
            let mut map = self.inner.write();
            let w = map
                .get_mut(worker_id)
                .ok_or_else(|| format!("worker not found: {}", worker_id))?;
            w.status = status;
        }
        self.persist();
        Ok(())
    }

    pub fn update_progress(&self, worker_id: &str, text: String) -> Result<(), String> {
        {
            let mut map = self.inner.write();
            let w = map
                .get_mut(worker_id)
                .ok_or_else(|| format!("worker not found: {}", worker_id))?;
            w.progress_text = Some(text);
        }
        self.persist();
        Ok(())
    }

    pub fn touch(&self, worker_id: &str, mem_id: &str) {
        let mut map = self.inner.write();
        if let Some(w) = map.get_mut(worker_id) {
            w.touched_nodes.insert(mem_id.to_string());
        }
        drop(map);
        self.persist();
    }

    pub fn set_tab_id(&self, worker_id: &str, tab_id: String) {
        let mut map = self.inner.write();
        if let Some(w) = map.get_mut(worker_id) {
            w.tab_id = tab_id;
        }
        drop(map);
        self.persist();
    }

    pub fn set_log_path(&self, worker_id: &str, path: String) {
        let mut map = self.inner.write();
        if let Some(w) = map.get_mut(worker_id) {
            w.log_path = Some(path);
        }
        drop(map);
        self.persist();
    }

    pub fn set_run_session(&self, worker_id: &str, session_id: String, cwd: Option<String>) {
        {
            let mut map = self.inner.write();
            match map.get_mut(worker_id) {
                Some(w) => {
                    w.session_id = Some(session_id);
                    if cwd.is_some() {
                        w.cwd = cwd;
                    }
                }
                None => return,
            }
        }
        self.persist();
    }

    pub fn complete(&self, worker_id: &str, report: String) -> Result<GraphWorkerInfo, String> {
        let info = {
            let mut map = self.inner.write();
            let w = map
                .get_mut(worker_id)
                .ok_or_else(|| format!("worker not found: {}", worker_id))?;
            w.status = WorkerStatus::Done;
            w.report = Some(report);
            w.done_at = Some(now_ms());
            w.clone()
        };
        self.persist();
        Ok(info)
    }

    pub fn kill(&self, worker_id: &str) -> Result<GraphWorkerInfo, String> {
        let info = {
            let mut map = self.inner.write();
            let w = map
                .get_mut(worker_id)
                .ok_or_else(|| format!("worker not found: {}", worker_id))?;
            if !w.status.is_terminal() {
                w.status = WorkerStatus::Done;
            }
            if w.done_at.is_none() {
                w.done_at = Some(now_ms());
            }
            w.clone()
        };
        self.persist();
        Ok(info)
    }
}

fn load_worker_records(
    path: &std::path::Path,
    text: &str,
    inner: &mut HashMap<String, GraphWorkerInfo>,
) {
    let records: serde_json::Value = match serde_json::from_str(text) {
        Ok(records) => records,
        Err(error) => {
            log::error!(
                "[graph-workers] quarantined unreadable store {}: {}; source left unchanged",
                path.display(),
                error
            );
            return;
        }
    };
    let Some(records) = records.as_array() else {
        log::error!(
            "[graph-workers] store {} is not an array; source left unchanged",
            path.display()
        );
        return;
    };
    for (index, record) in records.iter().enumerate() {
        match serde_json::from_value::<GraphWorkerInfo>(record.clone()) {
            Ok(mut item) => {
                if !item.status.is_terminal() {
                    item.status = WorkerStatus::Orphaned;
                    if item.done_at.is_none() {
                        item.done_at = Some(now_ms());
                    }
                }
                inner.insert(item.worker_id.clone(), item);
            }
            Err(error) => log::error!(
                "[graph-workers] quarantined record at index {} in {}: {}; source left unchanged",
                index,
                path.display(),
                error
            ),
        }
    }
}

/// Save worker PTY output to a log file. Returns the path on success.
pub fn save_worker_log(worker_id: &str, output: &str) -> Result<PathBuf, String> {
    let dir = codeterm_paths::data_dir().join("mem").join("workers");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {}", dir.display(), e))?;
    let path = dir.join(format!("{}.log", worker_id));
    // Use worker PTY dimensions for VT100 parsing so UI lines render at correct width.
    let clean = codeterm_util::terminal::clean_terminal_output_sized(output, 500, 120);
    std::fs::write(&path, clean).map_err(|e| format!("write {}: {}", path.display(), e))?;
    Ok(path)
}

pub fn make_info(
    worker_id: String,
    tab_id: String,
    intent: String,
    spawned_by: String,
    color: String,
) -> GraphWorkerInfo {
    GraphWorkerInfo {
        worker_id,
        tab_id,
        intent,
        spawned_by,
        spawned_at: now_ms(),
        status: WorkerStatus::Starting,
        progress_text: None,
        touched_nodes: HashSet::new(),
        color,
        report: None,
        done_at: None,
        log_path: None,
        agent_provider: None,
        model: None,
        session_id: None,
        cwd: None,
        commands: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("workers-{}-{}.json", name, uuid::Uuid::new_v4()))
    }

    fn fresh_info(id: &str) -> GraphWorkerInfo {
        make_info(
            id.into(),
            "tab1".into(),
            "test intent".into(),
            "user:test".into(),
            "hsl(0,0%,0%)".into(),
        )
    }

    #[test]
    fn register_list_get() {
        let reg = GraphWorkerRegistry::load(tmp_path("rlg"));
        reg.register(fresh_info("gw_aaaa"));
        reg.register(fresh_info("gw_bbbb"));
        assert_eq!(reg.list().len(), 2);
        assert!(reg.get("gw_aaaa").is_some());
        assert!(reg.get("gw_zzzz").is_none());
    }

    #[test]
    fn progress_and_complete() {
        let reg = GraphWorkerRegistry::load(tmp_path("pc"));
        reg.register(fresh_info("gw_x"));
        reg.update_progress("gw_x", "scanning 5/10".into()).unwrap();
        reg.touch("gw_x", "skill_a");
        reg.touch("gw_x", "fact_b");
        let info = reg.complete("gw_x", "all done".into()).unwrap();
        assert_eq!(info.status, WorkerStatus::Done);
        assert_eq!(info.report.as_deref(), Some("all done"));
        assert_eq!(info.progress_text.as_deref(), Some("scanning 5/10"));
        assert_eq!(info.touched_nodes.len(), 2);
    }

    #[test]
    fn persist_marks_orphaned_on_reload() {
        let path = tmp_path("orph");
        let reg = GraphWorkerRegistry::load(path.clone());
        let mut alive = fresh_info("gw_alive");
        alive.status = WorkerStatus::Reading;
        let mut done = fresh_info("gw_done");
        done.status = WorkerStatus::Done;
        done.done_at = Some(123);
        reg.register(alive);
        reg.register(done);
        drop(reg);

        let reg2 = GraphWorkerRegistry::load(path);
        let alive_after = reg2.get("gw_alive").unwrap();
        let done_after = reg2.get("gw_done").unwrap();
        assert_eq!(alive_after.status, WorkerStatus::Orphaned);
        assert!(alive_after.done_at.is_some());
        assert_eq!(done_after.status, WorkerStatus::Done);
        assert_eq!(done_after.done_at, Some(123));
    }

    #[test]
    fn load_isolates_malformed_worker_records_without_rewriting_the_store() {
        let path = tmp_path("malformed-record");
        let valid = fresh_info("gw_valid");
        let fixture = serde_json::json!([
            valid,
            {
                "worker_id": "gw_malformed",
                "status": ["not", "a", "status"]
            }
        ])
        .to_string();
        std::fs::write(&path, &fixture).unwrap();

        let registry = GraphWorkerRegistry::load(path.clone());
        let workers = registry.list();
        assert_eq!(workers.len(), 1);
        assert_eq!(workers[0].worker_id, "gw_valid");
        assert_eq!(workers[0].status, WorkerStatus::Orphaned);
        assert_eq!(std::fs::read_to_string(path).unwrap(), fixture);
    }

    #[test]
    fn build_task_substitutes_fields() {
        let task = build_worker_task("dedup db", "gw_test", "merge dups");
        assert!(task.contains("Your intent: dedup db"));
        assert!(task.contains("Your worker id: gw_test"));
        assert!(task.contains("merge dups"));
        assert!(task.contains("codeterm mem worker report"));
    }

    #[test]
    fn set_run_session_persists_session_and_cwd() {
        let reg = GraphWorkerRegistry::load(tmp_path("sess"));
        reg.register(fresh_info("gw_s"));
        reg.set_run_session("gw_s", "sid-123".into(), Some("/work/dir".into()));
        let w = reg.get("gw_s").unwrap();
        assert_eq!(w.session_id.as_deref(), Some("sid-123"));
        assert_eq!(w.cwd.as_deref(), Some("/work/dir"));
        // Missing worker is a no-op, not a panic.
        reg.set_run_session("gw_missing", "x".into(), None);
    }

    #[test]
    fn worker_id_format() {
        let id = new_worker_id();
        assert!(id.starts_with("gw_"));
        assert_eq!(id.len(), 3 + 8);
    }

    #[test]
    fn active_by_intent_skips_terminal_workers() {
        let reg = GraphWorkerRegistry::load(tmp_path("intent"));

        let mut w1 = fresh_info("gw_run1");
        w1.intent = "dreaming:rem".into();
        w1.status = WorkerStatus::Reading;
        reg.register(w1);

        assert_eq!(
            reg.active_by_intent("dreaming:rem").as_deref(),
            Some("gw_run1")
        );
        assert!(reg.active_by_intent("extract:note_a").is_none());

        let _ = reg.complete("gw_run1", "done".into());
        assert!(reg.active_by_intent("dreaming:rem").is_none());

        let mut w2 = fresh_info("gw_run2");
        w2.intent = "extract:note_a".into();
        w2.status = WorkerStatus::Analyzing;
        reg.register(w2);
        assert_eq!(
            reg.active_by_intent("extract:note_a").as_deref(),
            Some("gw_run2")
        );
    }
}
