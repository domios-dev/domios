use std::sync::LazyLock;

use parking_lot::Mutex;
use tauri::Manager;

use crate::mem::{workers, SearchQuery};
use crate::multiplexer::types::TabCloseReason;
use crate::plugin::activity::{emit_plugin_activity, PluginActivity, PluginActivityRegistry};
use crate::secrets::secret_file::SecretFile;
use crate::service::spawn::{SpawnTabSpec, TabVisibility};
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

fn api_cfg_base_url(cfg: crate::config::schema::ApiConfig) -> String {
    let host = if cfg.host == "0.0.0.0" {
        "127.0.0.1".to_string()
    } else {
        cfg.host
    };
    format!("http://{}:{}", host, cfg.port)
}

// ── PluginHostCtx impl ───────────────────────────────────────────────────────

impl codeterm_plugin::PluginHostCtx for ServiceContext {
    fn runtime_launch_authorized(
        &self,
        binding: &codeterm_plugin::agent_runtime::RuntimeLaunchBinding,
    ) -> bool {
        super::agent_runtime::authority::verifies(
            self,
            &binding.tab_id,
            &binding.provider_id,
            &binding.binding_nonce,
        )
    }

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
        let ws = match self.create_workspace(opts.name, opts.external_root, false) {
            Ok(workspace) => workspace,
            Err(error) => return err(error),
        };
        serde_json::json!({ "workspaceId": ws.id }).to_string()
    }

    fn workspace_tabs_json(&self, workspace_id: &str) -> String {
        let workspaces = match self.list_workspaces() {
            Ok(workspaces) => workspaces,
            Err(error) => return err(error),
        };
        let tabs: Vec<serde_json::Value> = workspaces
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
        if kind == codeterm_plugin::agent_verbs::AGENT_COMMANDS_CONTRIBUTION_KIND {
            super::plugin_prompt::refresh(&self.agent_command_registry);
            super::plugin_skills::spawn_sync(self.mem.clone(), self.agent_command_registry.clone());
        }
        self.events
            .publish(crate::api::events::SystemEvent::ContributionsChanged {
                kind: kind.to_string(),
            });
    }

    fn event_sources_changed(&self) {
        self.contributions_changed("graph-nodes");
        if let Some(handle) = self
            .app_handle
            .try_state::<crate::sentinel::scheduler::Handle>()
        {
            handle.notify();
        }
    }

    fn emit_graph_worker_spawned_json(&self, info_json: &str) {
        if let Ok(info) = serde_json::from_str::<workers::GraphWorkerInfo>(info_json) {
            crate::api::events::emit_everywhere(&self.app_handle, "graph-worker-spawned", &info);
        }
    }

    fn emit_graph_event_json(&self, plugin_id: &str, event_json: &str) -> String {
        if event_json.len() > codeterm_plugin::graph_events::MAX_EVENT_BYTES {
            return serde_json::json!({"admitted":false,"reason":"oversize"}).to_string();
        }
        let event: serde_json::Value = match serde_json::from_str(event_json) {
            Ok(event) => event,
            Err(error) => return serde_json::json!({ "error": error.to_string() }).to_string(),
        };
        let Some(subscription_id) = event.get("subscriptionId").and_then(|value| value.as_str())
        else {
            return serde_json::json!({ "error": "subscriptionId is required" }).to_string();
        };
        let Some(token) = event.get("token").and_then(|value| value.as_str()) else {
            return serde_json::json!({ "error": "token is required" }).to_string();
        };
        let Some(event_id) = event.get("eventId").and_then(|value| value.as_str()) else {
            return serde_json::json!({ "error": "eventId is required" }).to_string();
        };
        let Some(payload) = event.get("payload") else {
            return serde_json::json!({ "error": "payload is required" }).to_string();
        };
        let payload_bytes = match serde_json::to_vec(payload) {
            Ok(bytes) => bytes.len(),
            Err(error) => return serde_json::json!({ "error": error.to_string() }).to_string(),
        };
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis().min(u128::from(u64::MAX)) as u64)
            .unwrap_or_default();
        let decision =
            self.graph_event_subscriptions
                .admit(codeterm_plugin::graph_events::AdmissionRequest {
                    subscription_id,
                    token,
                    plugin_id,
                    event_id,
                    payload,
                    payload_bytes,
                    now_ms,
                });
        match decision {
            codeterm_plugin::graph_events::AdmissionDecision::Admit => {
                if let Some(pipeline_id) = self
                    .graph_event_subscriptions
                    .pipeline_id_for(subscription_id)
                {
                    if let Some(handle) = self
                        .app_handle
                        .try_state::<crate::sentinel::scheduler::Handle>()
                    {
                        handle.notify_event_subscription_changed(&pipeline_id);
                    }
                }
                serde_json::json!({ "admitted": true }).to_string()
            }
            codeterm_plugin::graph_events::AdmissionDecision::Refuse(reason) => {
                if let Some(pipeline_id) = self
                    .graph_event_subscriptions
                    .pipeline_id_for(subscription_id)
                {
                    if let Some(handle) = self
                        .app_handle
                        .try_state::<crate::sentinel::scheduler::Handle>()
                    {
                        handle.notify_event_subscription_changed(&pipeline_id);
                    }
                }
                let reason = match reason {
                    codeterm_plugin::graph_events::AdmitRefusal::UnknownSubscription => {
                        "unknown_subscription"
                    }
                    codeterm_plugin::graph_events::AdmitRefusal::BadToken => "bad_token",
                    codeterm_plugin::graph_events::AdmitRefusal::ForeignPlugin => "foreign_plugin",
                    codeterm_plugin::graph_events::AdmitRefusal::InvalidPayload => {
                        "invalid_payload"
                    }
                    codeterm_plugin::graph_events::AdmitRefusal::Oversize => "oversize",
                    codeterm_plugin::graph_events::AdmitRefusal::Duplicate => "duplicate",
                    codeterm_plugin::graph_events::AdmitRefusal::Backpressure => "backpressure",
                    codeterm_plugin::graph_events::AdmitRefusal::RateLimited => "rate_limited",
                };
                serde_json::json!({ "admitted": false, "reason": reason }).to_string()
            }
        }
    }

    // ── Socket / API ─────────────────────────────────────────────────────

    fn socket_display_path(&self) -> String {
        crate::socket::server::socket_display_path()
    }

    fn api_base_url(&self) -> String {
        api_cfg_base_url(self.configuration().api)
    }

    fn api_token(&self) -> Option<String> {
        self.configuration().api.token
    }

    fn model_provider_process_env(
        &self,
        plugin_id: &str,
    ) -> Result<
        std::collections::HashMap<String, String>,
        codeterm_plugin::host_traits::ModelProviderEnvUnavailable,
    > {
        self.model_provider_launch_env(plugin_id)
            .map_err(|_| codeterm_plugin::host_traits::ModelProviderEnvUnavailable)
    }

    fn supporter_token(&self, plugin_id: &str) -> Option<String> {
        stored_supporter_token(
            plugin_id,
            &crate::api::mem_routes::decision::supporter_token_dir(),
        )
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

    // ── Surfaces ─────────────────────────────────────────────────────────

    fn surface_call_json(
        &self,
        plugin_id: &str,
        caller: &str,
        verb: &str,
        args_json: &str,
    ) -> String {
        self.surface_agent_call_json(plugin_id, Some(caller), verb, args_json)
    }

    // ── Receiver target ──────────────────────────────────────────────────

    fn receiver_target_json(&self, plugin_id: &str) -> String {
        crate::service::receiver_target::host_json(self.receiver_target(plugin_id))
    }

    fn receiver_target_clear_json(&self, plugin_id: &str) -> String {
        use crate::service::receiver_target::{host_json, ReceiverTargetError};
        if !self.receiver_target_eligible(plugin_id) {
            return host_json(Err(ReceiverTargetError::ReceiverNotFound));
        }
        host_json(self.clear_receiver_target(plugin_id))
    }

    fn receiver_status_report_json(
        &self,
        plugin_id: &str,
        query: &codeterm_plugin::local_recording::ReceiverStatusQuery,
    ) -> String {
        super::local_recording::host_json(self.read_receiver_status_report(plugin_id, query))
    }

    fn local_recording_inputs_json(&self, plugin_id: &str) -> String {
        super::local_recording::host_json(self.local_recording_inputs(plugin_id))
    }

    fn local_recording_select_json(
        &self,
        plugin_id: &str,
        devices: codeterm_plugin::local_recording::SelectedDevices,
    ) -> String {
        super::local_recording::host_json(self.local_recording_select_devices(plugin_id, devices))
    }

    fn local_recording_saved_selection_json(&self, plugin_id: &str) -> String {
        super::local_recording::host_json((|| {
            if !self.local_recording_status(plugin_id)?.granted {
                return Err("local_recording_grant_required".into());
            }
            self.local_recording_selections.get(plugin_id)
        })())
    }

    fn local_recording_status_json(&self, plugin_id: &str) -> String {
        super::local_recording::host_json(self.local_recording_status(plugin_id))
    }

    fn local_recording_enabled_json(&self, plugin_id: &str, enabled: bool) -> String {
        super::local_recording::host_json(self.local_recording_set_enabled(plugin_id, enabled))
    }
}

fn stored_supporter_token(plugin_id: &str, token_dir: &std::path::Path) -> Option<String> {
    let store = crate::mem::decision::token_store::TokenStore::new(token_dir.to_path_buf());
    if store.status(plugin_id).ok()?.mode_ok == Some(false) {
        return None;
    }
    store
        .load(plugin_id)
        .ok()
        .flatten()
        .map(|token| token.expose().to_string())
}

#[cfg(test)]
mod supporter_token_tests {
    use super::*;

    #[test]
    fn host_reads_only_the_plugin_id_supporter_store_entry() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::mem::decision::token_store::TokenStore::new(dir.path().to_path_buf());
        store.set("decision-plugin", "stored-fixture").unwrap();

        assert_eq!(
            stored_supporter_token("decision-plugin", dir.path()).as_deref(),
            Some("stored-fixture")
        );
        assert_eq!(stored_supporter_token("another-plugin", dir.path()), None);
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
            let spec = plugin_worker_spawn_spec(
                crate::service::SpawnAgentParams {
                    agent: Some(provider),
                    model: (!model.is_empty()).then_some(model),
                    task: Some(task),
                    title: Some(title),
                    workspace_id,
                    resume_session_id,
                    audience: crate::multiplexer::types::TabAudience::Automation,
                    ..Default::default()
                },
                worker_id_blocking,
                spawned_by,
            );
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

fn plugin_worker_spawn_spec(
    params: crate::service::SpawnAgentParams,
    worker_id: String,
    spawned_by: String,
) -> SpawnTabSpec {
    let mut spec = SpawnTabSpec::from_agent_params(params);
    spec.visibility = TabVisibility::Hidden;
    if let Some(agent) = spec.agent_launch_mut() {
        agent.worker_id = Some(worker_id);
        agent.spawned_by = Some(spawned_by);
    }
    spec
}

#[cfg(test)]
mod worker_spawn_tests {
    use super::*;

    #[test]
    fn plugin_worker_runtime_keeps_original_params_and_worker_identity() {
        let params = crate::service::SpawnAgentParams {
            agent: Some("runtime-fixture".into()),
            model: Some("opaque-model".into()),
            task: Some("opaque-task".into()),
            workspace_id: Some("opaque-workspace".into()),
            resume_session_id: Some("opaque-session".into()),
            audience: crate::multiplexer::types::TabAudience::Automation,
            ..Default::default()
        };
        let expected = params.clone();
        let mut spec =
            plugin_worker_spawn_spec(params, "worker-exact".into(), "plugin-exact".into());
        spec.route_agent_to_chatbackend().unwrap();
        assert_eq!(spec.visibility, TabVisibility::Hidden);
        let metadata = spec.chatbackend_launch.unwrap();
        let mut actual = metadata.params.clone();
        let prompt = actual.system_prompt.take().unwrap();
        assert_eq!(prompt.matches("opaque-task").count(), 1);
        assert!(prompt.contains(crate::service::orchestrator::ROLE_AUTHORITY));
        assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
        assert_eq!(metadata.agent.worker_id.as_deref(), Some("worker-exact"));
        assert_eq!(metadata.agent.spawned_by.as_deref(), Some("plugin-exact"));
    }
}
