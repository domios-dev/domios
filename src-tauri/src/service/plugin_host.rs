use std::sync::LazyLock;

use parking_lot::Mutex;

use crate::mem::{workers, SearchQuery};
use crate::multiplexer::types::TabCloseReason;
use crate::plugin::activity::{emit_plugin_activity, PluginActivity, PluginActivityRegistry};
use crate::secrets::secret_file::SecretFile;
use crate::service::spawn::{AgentSpawnSpec, SpawnTabSpec, TabCommand, TabVisibility};
use crate::service::ServiceContext;

// ── Progress activity id map (mirrors host_compute) ─────────────────────────

static PROGRESS_ACTIVITY_IDS: LazyLock<Mutex<std::collections::HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

// ── Helpers ──────────────────────────────────────────────────────────────────

fn err(msg: impl Into<String>) -> String {
    serde_json::json!({ "error": msg.into() }).to_string()
}

#[derive(serde::Deserialize)]
struct ProgressOpts {
    label: String,
    #[serde(default)]
    percent: Option<f32>,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    error: Option<String>,
}

fn apply_progress_raw(
    registry: &PluginActivityRegistry,
    current_id: &mut Option<String>,
    plugin_id: &str,
    opts_json: &str,
) -> Result<PluginActivity, String> {
    let opts: ProgressOpts =
        serde_json::from_str(opts_json).map_err(|e| format!("bad progress opts: {e}"))?;
    if opts.label.trim().is_empty() {
        return Err("progress label is required".to_string());
    }
    let mut activity = match current_id
        .as_ref()
        .filter(|id| registry.get(id).is_some())
        .cloned()
    {
        Some(id) => registry.step(&id, &opts.label, opts.percent),
        None => {
            let started = registry.start(plugin_id, "bootstrap", &opts.label);
            *current_id = Some(started.id.clone());
            if opts.percent.is_some() {
                registry.step(&started.id, &opts.label, opts.percent)
            } else {
                started
            }
        }
    };
    if opts.done || opts.error.is_some() {
        activity = registry.finish(&activity.id, opts.error);
        *current_id = None;
    }
    Ok(activity)
}

fn search_hits_json(hits: &[crate::mem::SearchHit]) -> serde_json::Value {
    serde_json::json!({
        "hits": hits.iter().map(|h| serde_json::json!({
            "id": h.item.id,
            "text": h.item.body,
            "score": h.score,
        })).collect::<Vec<_>>()
    })
}

fn secret_bucket(plugin_id: &str) -> SecretFile {
    crate::secrets::plugin_secret_bucket(plugin_id)
}

fn api_cfg_base_url() -> String {
    let cfg = crate::config::load_config().api;
    let host = if cfg.host == "0.0.0.0" {
        "127.0.0.1".to_string()
    } else {
        cfg.host
    };
    format!("http://{}:{}", host, cfg.port)
}

// ── PluginHostCtx impl ───────────────────────────────────────────────────────

impl codeterm_plugin::PluginHostCtx for ServiceContext {
    // ── Memory ───────────────────────────────────────────────────────────

    fn mem_search_json(&self, opts_json: &str) -> String {
        #[derive(serde::Deserialize)]
        struct Opts {
            query: String,
            #[serde(default)]
            k: Option<usize>,
        }
        let opts: Opts = match serde_json::from_str(opts_json) {
            Ok(o) => o,
            Err(e) => return err(format!("bad mem.search opts: {e}")),
        };
        let q = SearchQuery {
            query: opts.query,
            kind: None,
            scope: None,
            area: None,
            limit: opts.k,
        };
        match self.mem.search(&q) {
            Ok(hits) => search_hits_json(&hits).to_string(),
            Err(e) => err(format!("mem.search: {e}")),
        }
    }

    // ── Analytics ────────────────────────────────────────────────────────

    fn analytics_query_json(&self, query_json: &str) -> String {
        self.analytics_snapshot_json(query_json)
    }

    // ── Workspace / tab ─────────────────────────────────────────────────

    fn workspace_ensure_json(&self, opts_json: &str) -> String {
        #[derive(serde::Deserialize)]
        struct Opts {
            name: String,
            #[serde(default, rename = "externalRoot")]
            external_root: Option<String>,
        }
        let opts: Opts = match serde_json::from_str(opts_json) {
            Ok(o) => o,
            Err(e) => return err(format!("bad workspace.ensure opts: {e}")),
        };
        let ws = self.create_workspace(opts.name, opts.external_root, false);
        serde_json::json!({ "workspaceId": ws.id }).to_string()
    }

    fn workspace_tabs_json(&self, workspace_id: &str) -> String {
        let tabs: Vec<serde_json::Value> = self
            .list_workspaces()
            .into_iter()
            .find(|w| w.id == workspace_id)
            .map(|w| {
                w.tab_ids
                    .iter()
                    .map(|p| serde_json::json!({ "tabId": p }))
                    .collect()
            })
            .unwrap_or_default();
        serde_json::Value::Array(tabs).to_string()
    }

    fn close_tab(&self, tab_id: &str) {
        let _ = ServiceContext::close_tab(self, tab_id);
    }

    fn spawn_tab_json(&self, _spec_json: &str) -> String {
        // Stub: full JSON-driven spawn spec parsing not yet wired.
        "{}".to_string()
    }

    // ── Worker ───────────────────────────────────────────────────────────

    fn worker_get_json(&self, session_id: &str) -> Option<String> {
        self.workers
            .get(session_id)
            .and_then(|w| serde_json::to_string(&w).ok())
    }

    fn worker_kill(&self, session_id: &str) {
        let _ = self.workers.kill(session_id);
    }

    fn worker_pick_color(&self) -> String {
        self.workers.pick_color()
    }

    fn worker_register_json(&self, info_json: &str) {
        match serde_json::from_str::<workers::GraphWorkerInfo>(info_json) {
            Ok(info) => self.workers.register(info),
            Err(e) => log::warn!("[plugin-host] worker_register_json bad payload: {e}"),
        }
    }

    fn worker_start_json(&self, plugin_id: &str, opts_json: &str) -> String {
        #[derive(serde::Deserialize)]
        struct Opts {
            task: String,
            #[serde(default)]
            provider: Option<String>,
            #[serde(default)]
            model: Option<String>,
        }
        let opts: Opts = match serde_json::from_str(opts_json) {
            Ok(o) => o,
            Err(e) => return err(format!("bad worker.start opts: {e}")),
        };
        let provider = opts.provider.unwrap_or_else(|| "claude".to_string());
        let model = opts.model.unwrap_or_default();
        let intent = format!(
            "plugin:{}:worker:{}",
            plugin_id,
            uuid::Uuid::new_v4().simple()
        );
        let spawned_by = format!("plugin:{plugin_id}");
        let svc = std::sync::Arc::new(self.clone());
        match workers::spawn_headless(&svc, intent, opts.task, spawned_by, provider, model) {
            Ok(job_id) => serde_json::json!({ "jobId": job_id }).to_string(),
            Err(e) => err(e),
        }
    }

    fn worker_poll_json(&self, session_id: &str) -> String {
        poll_envelope(self.workers.get(session_id).as_ref(), "report").to_string()
    }

    // ── Agent ────────────────────────────────────────────────────────────

    fn agent_spawn_json(&self, plugin_id: &str, workspace_id: &str, opts_json: &str) -> String {
        #[derive(serde::Deserialize)]
        struct Opts {
            task: String,
            #[serde(default)]
            provider: Option<String>,
            #[serde(default)]
            model: Option<String>,
            #[serde(default)]
            key: Option<String>,
            #[serde(default)]
            commands: Vec<String>,
        }
        let opts: Opts = match serde_json::from_str(opts_json) {
            Ok(o) => o,
            Err(e) => return err(format!("bad agent.spawn opts: {e}")),
        };
        let provider = opts.provider.unwrap_or_else(|| "claude".to_string());
        let model = opts.model.unwrap_or_default();
        let key = opts
            .key
            .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
        let intent = format!("plugin:{plugin_id}:agent:{key}");
        let spawned_by = format!("plugin:{plugin_id}");
        let svc = std::sync::Arc::new(self.clone());
        let session_id = enqueue_agent_turn(
            &svc,
            intent,
            spawned_by,
            provider,
            model,
            opts.task,
            Some(workspace_id.to_string()),
            None,
            opts.commands,
        );
        serde_json::json!({ "sessionId": session_id }).to_string()
    }

    fn agent_send_json(&self, plugin_id: &str, session_id: &str, text: &str) -> String {
        let (provider, model, resume) = match self.workers.get(session_id) {
            Some(w) => (
                w.agent_provider.unwrap_or_else(|| "claude".to_string()),
                w.model.unwrap_or_default(),
                w.session_id.or_else(|| Some(session_id.to_string())),
            ),
            None => (
                "claude".to_string(),
                String::new(),
                Some(session_id.to_string()),
            ),
        };
        let intent = format!(
            "plugin:{}:send:{}",
            plugin_id,
            uuid::Uuid::new_v4().simple()
        );
        let spawned_by = format!("plugin:{plugin_id}");
        let svc = std::sync::Arc::new(self.clone());
        let ticket = enqueue_agent_turn(
            &svc,
            intent,
            spawned_by,
            provider,
            model,
            text.to_string(),
            None,
            resume,
            Vec::new(),
        );
        serde_json::json!({ "ticket": ticket }).to_string()
    }

    fn agent_reap_json(&self, session_id: &str) -> String {
        if let Some(w) = self.workers.get(session_id) {
            if !w.tab_id.is_empty() {
                let _ = ServiceContext::close_tab_with_reason(
                    self,
                    &w.tab_id,
                    TabCloseReason::Cancelled,
                );
            }
            let _ = self.workers.kill(session_id);
        }
        serde_json::json!({ "ok": true }).to_string()
    }

    fn agent_poll_json(&self, session_id: &str) -> String {
        poll_envelope(self.workers.get(session_id).as_ref(), "reply").to_string()
    }

    // ── Progress ─────────────────────────────────────────────────────────

    fn progress_raw_json(&self, plugin_id: &str, opts_json: &str) -> String {
        let activity = {
            let mut ids = PROGRESS_ACTIVITY_IDS.lock();
            let mut current = ids.get(plugin_id).cloned();
            let activity =
                match apply_progress_raw(&self.plugin_activity, &mut current, plugin_id, opts_json)
                {
                    Ok(a) => a,
                    Err(e) => {
                        log::warn!("[plugin-progress] {plugin_id}: {e}");
                        return "{}".to_string();
                    }
                };
            match current {
                Some(id) => {
                    ids.insert(plugin_id.to_string(), id);
                }
                None => {
                    ids.remove(plugin_id);
                }
            }
            activity
        };
        emit_plugin_activity(self, &activity);
        "{}".to_string()
    }

    // ── Events ───────────────────────────────────────────────────────────

    fn contributions_changed(&self, kind: &str) {
        self.events
            .publish(crate::api::events::SystemEvent::ContributionsChanged {
                kind: kind.to_string(),
            });
    }

    fn emit_graph_worker_spawned_json(&self, info_json: &str) {
        if let Ok(info) = serde_json::from_str::<workers::GraphWorkerInfo>(info_json) {
            crate::api::events::emit_everywhere(&self.app_handle, "graph-worker-spawned", &info);
        }
    }

    // ── Socket / API ─────────────────────────────────────────────────────

    fn socket_display_path(&self) -> String {
        crate::socket::server::socket_display_path()
    }

    fn api_base_url(&self) -> String {
        api_cfg_base_url()
    }

    fn api_token(&self) -> Option<String> {
        crate::config::load_config().api.token
    }

    // ── Secrets ──────────────────────────────────────────────────────────

    fn secret_get(&self, plugin_id: &str, name: &str) -> Option<String> {
        secret_bucket(plugin_id).get(name).ok()
    }

    fn secret_set(&self, plugin_id: &str, name: &str, value: &str) -> bool {
        secret_bucket(plugin_id).set(name, value).is_ok()
    }

    fn secret_delete(&self, plugin_id: &str, name: &str) -> bool {
        secret_bucket(plugin_id).delete(name).is_ok()
    }

    // ── Receiver target ──────────────────────────────────────────────────

    fn receiver_target_json(&self, plugin_id: &str) -> String {
        crate::service::receiver_target::host_json(self.receiver_target(plugin_id))
    }

    fn receiver_target_clear_json(&self, plugin_id: &str) -> String {
        use crate::service::receiver_target::{host_json, ReceiverTargetError};
        if !self.webhook_registry.contains(plugin_id) {
            return host_json(Err(ReceiverTargetError::ReceiverNotFound));
        }
        host_json(self.clear_receiver_target(plugin_id))
    }
}

// ── Poll envelope (shared by worker_poll / agent_poll) ───────────────────────

fn poll_envelope(info: Option<&workers::GraphWorkerInfo>, ok_field: &str) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    match info {
        None => {
            m.insert("done".into(), true.into());
            m.insert("error".into(), "job not found".into());
        }
        Some(w) => match w.status {
            workers::WorkerStatus::Done => {
                m.insert("done".into(), true.into());
                m.insert(ok_field.into(), w.report.clone().unwrap_or_default().into());
            }
            workers::WorkerStatus::Error | workers::WorkerStatus::Orphaned => {
                m.insert("done".into(), true.into());
                let reason = w
                    .report
                    .clone()
                    .unwrap_or_else(|| format!("worker {:?}", w.status));
                m.insert("error".into(), reason.into());
            }
            _ => {
                m.insert("done".into(), false.into());
            }
        },
    }
    serde_json::Value::Object(m)
}

// ── Agent turn enqueue (mirrors host_compute::enqueue_agent_turn) ─────────────

const TURN_WATCHDOG_SECS: u64 = 30 * 60;

#[allow(clippy::too_many_arguments)]
fn enqueue_agent_turn(
    svc: &std::sync::Arc<ServiceContext>,
    intent: String,
    spawned_by: String,
    provider: String,
    model: String,
    task_prompt: String,
    workspace_id: Option<String>,
    resume_session_id: Option<String>,
    commands: Vec<String>,
) -> String {
    let worker_id = workers::new_worker_id();
    let color = svc.workers.pick_color();
    let mut info = workers::make_info(
        worker_id.clone(),
        String::new(),
        intent.clone(),
        spawned_by.clone(),
        color,
    );
    info.status = workers::WorkerStatus::Queued;
    info.agent_provider = Some(provider.clone());
    if !model.is_empty() {
        info.model = Some(model.clone());
    }
    info.commands = commands;
    svc.workers.register(info.clone());
    crate::api::events::emit_everywhere(&svc.app_handle, "graph-worker-spawned", &info);

    let svc_task = svc.clone();
    let worker_id_task = worker_id.clone();
    let provider_acquire = provider.clone();
    tauri::async_runtime::spawn(async move {
        let permit = svc_task.worker_pool.acquire(&provider_acquire).await;
        if svc_task
            .workers
            .get(&worker_id_task)
            .map(|w| w.status.is_terminal())
            .unwrap_or(true)
        {
            drop(permit);
            return;
        }
        let _ = svc_task
            .workers
            .update_status(&worker_id_task, workers::WorkerStatus::Starting);

        let task = crate::prompts::PLUGIN_AGENT_TASK_PREAMBLE.replace("{task}", &task_prompt);
        let svc_blocking = svc_task.clone();
        let worker_id_blocking = worker_id_task.clone();
        let title = format!("plugin:{intent}");
        let spawn_result = tokio::task::spawn_blocking(move || {
            let spec = SpawnTabSpec {
                visibility: TabVisibility::Hidden,
                audience: crate::multiplexer::types::TabAudience::Automation,
                title: Some(title),
                description: None,
                workspace_id,
                command: TabCommand::Agent(AgentSpawnSpec {
                    provider,
                    planned_model: None,
                    model: if model.is_empty() { None } else { Some(model) },
                    task: Some(task),
                    worker_id: Some(worker_id_blocking.clone()),
                    spawned_by: Some(spawned_by),
                    resume_session_id,
                    ..Default::default()
                }),
                ..Default::default()
            };
            svc_blocking.spawn_tab(spec)
        })
        .await;

        let handle = match spawn_result {
            Ok(Ok(h)) => h,
            Ok(Err(e)) => {
                log::error!("[plugin-host] {worker_id_task} spawn_tab failed: {e}");
                let _ = svc_task
                    .workers
                    .update_status(&worker_id_task, workers::WorkerStatus::Error);
                drop(permit);
                return;
            }
            Err(e) => {
                log::error!("[plugin-host] {worker_id_task} spawn_blocking panicked: {e}");
                let _ = svc_task
                    .workers
                    .update_status(&worker_id_task, workers::WorkerStatus::Error);
                drop(permit);
                return;
            }
        };

        svc_task
            .workers
            .set_tab_id(&worker_id_task, handle.tab_id.clone());

        let deadline =
            tokio::time::Instant::now() + std::time::Duration::from_secs(TURN_WATCHDOG_SECS);
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            let Some(current) = svc_task.workers.get(&worker_id_task) else {
                break;
            };
            if current.status.is_terminal() {
                break;
            }
            if tokio::time::Instant::now() > deadline {
                log::warn!("[plugin-host] {worker_id_task} exceeded watchdog, orphaning");
                let _ = svc_task
                    .workers
                    .update_status(&worker_id_task, workers::WorkerStatus::Orphaned);
                let _ = svc_task.close_tab_with_reason(&handle.tab_id, TabCloseReason::Watchdog);
                break;
            }
        }
        drop(permit);
    });

    worker_id
}
