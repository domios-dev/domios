use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub const WORKER_TASK_PREAMBLE: &str = r#"You are a Graph Worker for the CodeTerm memory system.
Your intent: {intent}
Your worker id: {worker_id}

You are NOT visible as a regular tab — you exist in the memory panel only.
Report progress via `codeterm mem worker progress "<status>"`.
When finished, run `codeterm mem worker report "<summary>"` and exit.

TASK:
{task}
"#;

pub fn build_worker_task(intent: &str, worker_id: &str, task: &str) -> String {
    WORKER_TASK_PREAMBLE
        .replace("{intent}", intent)
        .replace("{worker_id}", worker_id)
        .replace("{task}", task)
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphWorkerInfo {
    pub worker_id: String,
    pub pane_id: String,
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
                Ok(text) => match serde_json::from_str::<Vec<GraphWorkerInfo>>(&text) {
                    Ok(items) => {
                        for mut item in items {
                            if !item.status.is_terminal() {
                                item.status = WorkerStatus::Orphaned;
                                if item.done_at.is_none() {
                                    item.done_at = Some(now_ms());
                                }
                            }
                            inner.insert(item.worker_id.clone(), item);
                        }
                    }
                    Err(e) => log::warn!("[graph-workers] parse {}: {}", persist_path.display(), e),
                },
                Err(e) => log::warn!("[graph-workers] read {}: {}", persist_path.display(), e),
            }
        }
        let reg = Arc::new(Self {
            inner: RwLock::new(inner),
            persist_path,
        });
        reg.persist();
        reg
    }

    fn persist(&self) {
        // Evict oldest done workers beyond MAX_HISTORY, delete their log files
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

    pub fn set_log_path(&self, worker_id: &str, path: String) {
        let mut map = self.inner.write();
        if let Some(w) = map.get_mut(worker_id) {
            w.log_path = Some(path);
        }
        drop(map);
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

/// Save worker PTY output to a log file. Returns the path on success.
pub fn save_worker_log(worker_id: &str, output: &str) -> Result<PathBuf, String> {
    let dir = crate::paths::data_dir().join("mem").join("workers");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("create {}: {}", dir.display(), e))?;
    let path = dir.join(format!("{}.log", worker_id));
    // Use the VT100 parser with worker PTY dimensions (120x40) — matches the
    // actual terminal width so TUI elements (horizontal rules, status bars)
    // render at the correct column width.
    let clean = crate::pty::ansi::clean_terminal_output_sized(output, 500, 120);
    std::fs::write(&path, clean)
        .map_err(|e| format!("write {}: {}", path.display(), e))?;
    Ok(path)
}

pub fn make_info(
    worker_id: String,
    pane_id: String,
    intent: String,
    spawned_by: String,
    color: String,
) -> GraphWorkerInfo {
    GraphWorkerInfo {
        worker_id,
        pane_id,
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
    }
}

/// Spawn a fully headless worker: hidden pane, backend PTY, backend reader.
/// Used by extraction workers and dreaming. Returns worker_id or error.
pub fn spawn_headless(
    svc: &Arc<crate::service::ServiceContext>,
    intent: String,
    task_prompt: String,
    spawned_by: String,
    agent_provider: String,
    model: String,
) -> Result<String, String> {
    let worker_id = new_worker_id();
    let task = build_worker_task(&intent, &worker_id, &task_prompt);

    // Find workspace
    let ws_id = svc.mux.lock()
        .list_workspaces().first().map(|w| w.id.clone())
        .ok_or("no workspace")?;
    let ws_cwd = svc.mux.lock()
        .get_workspace(&ws_id).and_then(|ws| ws.cwd.clone());

    // Create hidden pane
    let (_tab_id, pane_id) = {
        let mut mux = svc.mux.lock();
        let result = mux.create_tab(&ws_id)?;
        if let Some(pane) = mux.get_pane_mut(&result.1) {
            pane.hidden = true;
            pane.show_in_tabbar = false;
            pane.worker_id = Some(worker_id.clone());
            pane.auto_approve = true;
        }
        result
    };

    // Spawn PTY
    let mut env_vars: HashMap<String, String> = HashMap::new();
    env_vars.insert("CODETERM_PANE_ID".into(), pane_id.clone());
    env_vars.insert("CODETERM_WORKER_ID".into(), worker_id.clone());
    env_vars.insert("CODETERM_SOCKET_PATH".into(), crate::socket::server::socket_display_path());
    let api_cfg = crate::config::load_config().api;
    let host = if api_cfg.host == "0.0.0.0" { "127.0.0.1".to_string() } else { api_cfg.host.clone() };
    env_vars.insert("CODETERM_API_URL".into(), format!("http://{}:{}", host, api_cfg.port));
    if let Some(ref tok) = api_cfg.token {
        env_vars.insert("CODETERM_API_TOKEN".into(), tok.clone());
    }

    let pty_id = svc.pty.lock()
        .spawn(None, ws_cwd, 120, 40, env_vars)
        .map(|(id, _)| id)?;

    svc.mux.lock().link_pty(&pane_id, pty_id.clone());
    svc.emit_state_changed();

    // Resolve model args
    let agent_registry = svc.agent_registry.clone();
    let model_args: Vec<String> = agent_registry
        .with_provider(&agent_provider, |p| p.model_launch_args(&model))
        .unwrap_or_default();

    // Register worker
    let color = svc.workers.pick_color();
    let mut info = make_info(worker_id.clone(), pane_id.clone(), intent, spawned_by, color);
    info.agent_provider = Some(agent_provider.clone());
    if !model.is_empty() {
        info.model = Some(model);
    }
    svc.workers.register(info.clone());
    let _ = tauri::Emitter::emit(&svc.app_handle, "graph-worker-spawned", &info);

    log::info!("[worker] headless {} pane={} pty={}", worker_id, pane_id, pty_id);

    // Background: reader + launch agent
    let svc2 = svc.clone();
    let pane_id2 = pane_id.clone();
    let pty_id2 = pty_id.clone();
    std::thread::spawn(move || {
        let svc3 = svc2.clone();
        let pty_id3 = pty_id2.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all().build().expect("backend reader runtime");
            rt.block_on(async {
                match crate::service::pty_lifecycle::start_backend_reader(&svc3, &pty_id3) {
                    Ok(handle) => { let _ = handle.await; }
                    Err(e) => log::warn!("[worker] backend reader: {}", e),
                }
            });
        });

        crate::service::pty_io::poll_shell_prompt(
            &svc2.screen_state, &pty_id2,
            std::time::Duration::from_secs(10),
        );

        let launch_params = crate::agent::provider::AgentLaunchParams {
            agent_name: None,
            task: Some(task),
            args: if model_args.is_empty() { None } else { Some(model_args) },
            skip_permissions: true,
        };
        let launch_cmd = agent_registry
            .with_provider(&agent_provider, |p| p.build_launch_command(&launch_params))
            .unwrap_or_else(|| format!("{} --dangerously-skip-permissions", agent_provider));

        if let Err(e) = svc2.send_text(&pane_id2, &launch_cmd, true) {
            log::error!("[worker] send launch cmd: {}", e);
        }
    });

    Ok(worker_id)
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
            "pane1".into(),
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
    fn build_task_substitutes_fields() {
        let task = build_worker_task("dedup db", "gw_test", "merge dups");
        assert!(task.contains("Your intent: dedup db"));
        assert!(task.contains("Your worker id: gw_test"));
        assert!(task.contains("merge dups"));
        assert!(task.contains("codeterm mem worker report"));
    }

    #[test]
    fn worker_id_format() {
        let id = new_worker_id();
        assert!(id.starts_with("gw_"));
        assert_eq!(id.len(), 3 + 8);
    }
}
