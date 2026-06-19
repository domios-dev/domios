//! Host compute base — the `host.worker` / `host.agent` / `host.workspace` /
//! `host.mem` handles a plugin calls (plan §1.10, Track C2).
//!
//! Split out of `host_api.rs` so the flat bind table there stays a flat table
//! and this file owns the `ServiceContext`-backed logic.
//!
//! ## Sync vs async (the load-bearing distinction)
//!
//! The VM is single-threaded and `call_json` holds its mutex for the whole
//! call (`vm.rs`). So anything that could block for an agent **turn** MUST NOT
//! run inline — it would serialize every other call into that plugin (e.g. a
//! chatBackend `poll`). Those handles spawn a hidden agent pane via the worker
//! pool and return a **job id / ticket** the plugin polls (F4):
//!
//! * `host.worker.start` → `{ jobId }`, `host.worker.poll` → `{ done, report?, error? }`
//! * `host.agent.spawn/resume/get/send` → ids/tickets, `host.agent.poll` → `{ done, reply? }`
//!
//! All of these are **synchronous** in the VM preamble (`vm.rs`) — job-id/poll is
//! sync *per step* (each call returns immediately; the long-running work happens
//! on the host between calls). They are NOT Promise-wrapped: QuickJS can't drain
//! a Promise inside a synchronous `call_json` dispatch, so wrapping would break
//! sync consumers like the chatBackend `pump`/`poll` (Item 6). `await` on a
//! non-thenable still yields the value, so awaiting plugin code keeps working.
//!
//! mem search is likewise sync (`mem/manager_search.rs`), and the
//! `host.workspace` registry ops are mux operations, not agent turns.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use parking_lot::Mutex;
use rquickjs::function::Func;
use rquickjs::Object;

use super::host_api::PluginSandbox;
use super::vm::VmError;
use crate::mem::workers::{self, GraphWorkerInfo, WorkerStatus};
use crate::mem::{SearchHit, SearchQuery};
use crate::plugin::activity::{emit_plugin_activity, PluginActivity, PluginActivityRegistry};
use crate::service::spawn::{AgentSpawnSpec, PaneCommand, PaneVisibility, SpawnPaneSpec};
use crate::service::ServiceContext;

/// Watchdog ceiling for a plugin-spawned agent turn — mirrors the headless
/// worker watchdog (`mem/workers.rs`). A turn that never reports is forced
/// `Orphaned` and its hidden pane closed so it can't leak.
const TURN_WATCHDOG_SECS: u64 = 30 * 60;

static PROGRESS_ACTIVITY_IDS: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

// ── Pure helpers (unit-tested; no `ServiceContext`) ──────────────────────

/// JSON error envelope. Every sync bind returns a parseable object so the
/// plugin never sees a bare `null` it can't distinguish from "no result".
fn err(msg: impl Into<String>) -> String {
    serde_json::json!({ "error": msg.into() }).to_string()
}

/// Backend selector shared by `host.worker` / `host.agent` opts. Absent
/// fields fall back to the default agent provider.
#[derive(serde::Deserialize, Default)]
struct Backend {
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    model: Option<String>,
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

/// Resolve `(provider, model)` from an optional `Backend`, defaulting the
/// provider to `claude` and the model to empty (= provider default).
fn backend_provider_model(backend: &Option<Backend>) -> (String, String) {
    match backend {
        Some(b) => (
            b.provider.clone().unwrap_or_else(|| "claude".to_string()),
            b.model.clone().unwrap_or_default(),
        ),
        None => ("claude".to_string(), String::new()),
    }
}

/// Map mem `SearchHit`s to the SDK `{ hits: [{ id, text, score }] }` shape.
fn search_hits_json(hits: &[SearchHit]) -> serde_json::Value {
    serde_json::json!({
        "hits": hits
            .iter()
            .map(|h| serde_json::json!({
                "id": h.item.id,
                "text": h.item.body,
                "score": h.score,
            }))
            .collect::<Vec<_>>()
    })
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

/// `host.worker.poll` envelope: `{ done, report?, error? }`.
fn worker_poll_json(info: Option<&GraphWorkerInfo>) -> serde_json::Value {
    poll_envelope(info, "report")
}

/// `host.agent.poll` envelope: `{ done, reply? }` (the reply carries the
/// agent's reported text). Same job model as the worker poll.
fn agent_poll_json(info: Option<&GraphWorkerInfo>) -> serde_json::Value {
    poll_envelope(info, "reply")
}

/// Shared poll envelope. `ok_field` names the success payload key
/// (`report` for workers, `reply` for agents).
fn poll_envelope(info: Option<&GraphWorkerInfo>, ok_field: &str) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    match info {
        None => {
            m.insert("done".into(), true.into());
            m.insert("error".into(), "job not found".into());
        }
        Some(w) => match w.status {
            WorkerStatus::Done => {
                m.insert("done".into(), true.into());
                m.insert(ok_field.into(), w.report.clone().unwrap_or_default().into());
            }
            WorkerStatus::Error | WorkerStatus::Orphaned => {
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

// ── `ServiceContext`-backed handlers ─────────────────────────────────────

/// `host.mem.search` — synchronous (mem search is sync per F4).
fn mem_search(svc: &Arc<ServiceContext>, opts_json: &str) -> String {
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
    match svc.mem.search(&q) {
        Ok(hits) => search_hits_json(&hits).to_string(),
        Err(e) => err(format!("mem.search: {e}")),
    }
}

/// `host.workspace.ensure` — synchronous (mux op; dedupes by cwd).
fn workspace_ensure(svc: &Arc<ServiceContext>, opts_json: &str) -> String {
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
    let ws = svc.create_workspace(opts.name, opts.external_root, false);
    serde_json::json!({ "workspaceId": ws.id }).to_string()
}

/// `host.workspace.panes` — synchronous. Missing workspace → empty array so
/// the SDK's `PaneRef[]` contract holds (a plugin sees "no panes", not an error
/// object it would have to type-guard).
fn workspace_panes(svc: &Arc<ServiceContext>, workspace_id: &str) -> String {
    let panes: Vec<serde_json::Value> = svc
        .list_workspaces()
        .into_iter()
        .find(|w| w.id == workspace_id)
        .map(|w| {
            w.pane_ids
                .iter()
                .map(|p| serde_json::json!({ "paneId": p }))
                .collect()
        })
        .unwrap_or_default();
    serde_json::Value::Array(panes).to_string()
}

/// `host.worker.start` — async (one-shot agent compute). Returns `{ jobId }`
/// synchronously; the agent runs and reports out of band. Reuses the headless
/// worker spawn (no workspace target, no resume).
fn worker_start(svc: &Arc<ServiceContext>, plugin_id: &str, opts_json: &str) -> String {
    #[derive(serde::Deserialize)]
    struct Opts {
        task: String,
        #[serde(default)]
        backend: Option<Backend>,
    }
    let opts: Opts = match serde_json::from_str(opts_json) {
        Ok(o) => o,
        Err(e) => return err(format!("bad worker.start opts: {e}")),
    };
    let (provider, model) = backend_provider_model(&opts.backend);
    let intent = format!(
        "plugin:{}:worker:{}",
        plugin_id,
        uuid::Uuid::new_v4().simple()
    );
    let spawned_by = format!("plugin:{plugin_id}");
    match workers::spawn_headless(svc, intent, opts.task, spawned_by, provider, model) {
        Ok(job_id) => serde_json::json!({ "jobId": job_id }).to_string(),
        Err(e) => err(e),
    }
}

fn progress_raw(svc: &Arc<ServiceContext>, plugin_id: &str, opts_json: &str) -> String {
    let activity = {
        let mut ids = PROGRESS_ACTIVITY_IDS.lock();
        let mut current = ids.get(plugin_id).cloned();
        let activity =
            match apply_progress_raw(&svc.plugin_activity, &mut current, plugin_id, opts_json) {
                Ok(activity) => activity,
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
    emit_plugin_activity(svc, &activity);
    "{}".to_string()
}

/// `host.agent.spawn` — async. Starts a FRESH session in `workspace_id` and
/// returns `{ sessionId }` (the job id the plugin uses for send/poll/reap). The
/// underlying provider session id is captured out of band; the live round-trip
/// is exercised at integration (Track F / Track V).
fn agent_spawn(
    svc: &Arc<ServiceContext>,
    plugin_id: &str,
    workspace_id: &str,
    opts_json: &str,
) -> String {
    #[derive(serde::Deserialize)]
    struct Opts {
        task: String,
        #[serde(default)]
        backend: Option<Backend>,
        #[serde(default)]
        key: Option<String>,
        #[serde(default)]
        commands: Vec<String>,
    }
    let opts: Opts = match serde_json::from_str(opts_json) {
        Ok(o) => o,
        Err(e) => return err(format!("bad agent.spawn opts: {e}")),
    };
    let (provider, model) = backend_provider_model(&opts.backend);
    let key = opts
        .key
        .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
    let intent = format!("plugin:{plugin_id}:agent:{key}");
    let spawned_by = format!("plugin:{plugin_id}");
    let session_id = enqueue_agent_turn(
        svc,
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

/// `host.agent.resume` — re-attach a SPECIFIC stored session id. The host keeps
/// no live session pool (the plugin owns its LRU map per R2), so this echoes the
/// id back as the handle to use on the next `send`. Real re-attachment lands at
/// integration.
fn agent_resume_json(session_id: &str) -> String {
    serde_json::json!({ "sessionId": session_id }).to_string()
}

/// `host.agent.get` — the plugin owns its own `key → session` map (R2), so the
/// host has nothing to look up: always `null`.
fn agent_get_json() -> String {
    "null".to_string()
}

/// `host.agent.send` — async. Delivers `text` to the session (resuming its
/// captured provider session) as a one-shot turn and returns `{ ticket }`; the
/// plugin polls the ticket for the reply.
fn agent_send(svc: &Arc<ServiceContext>, plugin_id: &str, session_id: &str, text: &str) -> String {
    let (provider, model, resume) = match svc.workers.get(session_id) {
        Some(w) => (
            w.agent_provider.unwrap_or_else(|| "claude".to_string()),
            w.model.unwrap_or_default(),
            // Resume the captured provider session if known, else treat the
            // handle itself as the session id (forward-compatible).
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
    let ticket = enqueue_agent_turn(
        svc,
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

/// `host.agent.reap` — close the session's hidden pane and mark its job
/// terminal. Best-effort: an unknown id is a no-op.
fn agent_reap(svc: &Arc<ServiceContext>, session_id: &str) -> String {
    if let Some(w) = svc.workers.get(session_id) {
        if !w.pane_id.is_empty() {
            let _ = svc.close_pane(&w.pane_id);
        }
        let _ = svc.workers.kill(session_id);
    }
    serde_json::json!({ "ok": true }).to_string()
}

/// `host.agent.usage` — optional read-only stats. Not wired this track; the
/// SDK types it nullable, so `null` is the honest answer until the
/// `parseUsage`/`UsageSnapshot` bridge lands (R6).
fn agent_usage_json() -> String {
    "null".to_string()
}

/// Spawn a hidden agent pane as a tracked worker job and return its id
/// synchronously. Mirrors `mem/workers.rs::spawn_headless` (worker-pool permit,
/// `spawn_blocking(spawn_pane(Hidden, Agent{…}))`, watchdog) but carries a
/// workspace target + optional resume so it backs both `agent.spawn` (fresh)
/// and `agent.send` (resume). The agent reports via the worker-report path,
/// which drives `GraphWorkerRegistry::complete` → readable by `poll`.
///
/// The job runs OUTSIDE the VM lock: this returns the id immediately and the
/// spawn happens on the async runtime.
#[allow(clippy::too_many_arguments)]
fn enqueue_agent_turn(
    svc: &Arc<ServiceContext>,
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
    info.status = WorkerStatus::Queued;
    info.agent_provider = Some(provider.clone());
    if !model.is_empty() {
        info.model = Some(model.clone());
    }
    // C2 MINOR-2: scope the inverse seam to the verbs the plugin declared.
    info.commands = commands;
    svc.workers.register(info.clone());
    let _ = tauri::Emitter::emit(&svc.app_handle, "graph-worker-spawned", &info);

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
            .update_status(&worker_id_task, WorkerStatus::Starting);

        // Plugin agents own their full prompt and act through plugin verbs — do
        // NOT wrap them in the mem graph-worker preamble (that identity makes the
        // agent report via `mem worker` and never run the plugin's commands).
        let task = crate::prompts::PLUGIN_AGENT_TASK_PREAMBLE.replace("{task}", &task_prompt);
        let svc_blocking = svc_task.clone();
        let worker_id_blocking = worker_id_task.clone();
        let title = format!("plugin:{intent}");
        let spawn_result = tokio::task::spawn_blocking(move || {
            let spec = SpawnPaneSpec {
                visibility: PaneVisibility::Hidden,
                title: Some(title),
                description: None,
                workspace_id,
                command: PaneCommand::Agent(AgentSpawnSpec {
                    provider,
                    model: if model.is_empty() { None } else { Some(model) },
                    task: Some(task),
                    worker_id: Some(worker_id_blocking.clone()),
                    auto_approve: true,
                    spawned_by: Some(spawned_by),
                    resume_session_id,
                    ..Default::default()
                }),
                ..Default::default()
            };
            svc_blocking.spawn_pane(spec)
        })
        .await;

        let handle = match spawn_result {
            Ok(Ok(h)) => h,
            Ok(Err(e)) => {
                log::error!("[plugin-agent] {worker_id_task} spawn_pane failed: {e}");
                let _ = svc_task
                    .workers
                    .update_status(&worker_id_task, WorkerStatus::Error);
                drop(permit);
                return;
            }
            Err(e) => {
                log::error!("[plugin-agent] {worker_id_task} spawn_blocking panicked: {e}");
                let _ = svc_task
                    .workers
                    .update_status(&worker_id_task, WorkerStatus::Error);
                drop(permit);
                return;
            }
        };

        svc_task
            .workers
            .set_pane_id(&worker_id_task, handle.pane_id.clone());

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
                log::warn!("[plugin-agent] {worker_id_task} exceeded watchdog, orphaning");
                let _ = svc_task
                    .workers
                    .update_status(&worker_id_task, WorkerStatus::Orphaned);
                let _ = svc_task.close_pane(&handle.pane_id);
                break;
            }
        }
        drop(permit);
    });

    worker_id
}

// ── Bind table ───────────────────────────────────────────────────────────

/// Install the `host.worker*` / `host.agent*` / `host.workspace*` / `host.mem*`
/// raw binds onto the `host` object. The VM preamble (`vm.rs`) folds these into
/// the ergonomic `host.worker` / `host.agent` (Promise-wrapped) and
/// `host.workspace` / `host.mem` (sync) objects.
///
/// Every bind resolves the late-bound `ServiceContext` (`sandbox.service()`);
/// before service init it returns an error envelope rather than panicking.
pub(super) fn install_handles(
    host: &Object<'_>,
    sandbox: &Arc<PluginSandbox>,
) -> Result<(), VmError> {
    macro_rules! bind {
        ($name:expr, $closure:expr) => {
            host.set($name, Func::new($closure))
                .map_err(|e| VmError::Script(format!("set {}: {e}", $name)))?
        };
    }

    // ── host.worker (async job-id/poll) ──
    let sb = sandbox.clone();
    bind!("workerStart", move |opts: String| -> String {
        match sb.service() {
            Some(svc) => worker_start(&svc, sb.plugin_id(), &opts),
            None => err("host service unavailable"),
        }
    });
    let sb = sandbox.clone();
    bind!("workerPoll", move |job_id: String| -> String {
        match sb.service() {
            Some(svc) => worker_poll_json(svc.workers.get(&job_id).as_ref()).to_string(),
            None => err("host service unavailable"),
        }
    });

    let sb = sandbox.clone();
    bind!("progressRaw", move |opts: String| -> String {
        match sb.service() {
            Some(svc) => progress_raw(&svc, sb.plugin_id(), &opts),
            None => "{}".to_string(),
        }
    });

    // ── host.workspace (sync) — gated by the `manage_workspaces` grant (§1.10) ──
    let sb = sandbox.clone();
    bind!("workspaceEnsure", move |opts: String| -> String {
        if !sb.manage_workspaces_allowed() {
            return err("host.workspace: manage_workspaces permission required");
        }
        match sb.service() {
            Some(svc) => workspace_ensure(&svc, &opts),
            None => err("host service unavailable"),
        }
    });
    let sb = sandbox.clone();
    bind!("workspacePanes", move |workspace_id: String| -> String {
        if !sb.manage_workspaces_allowed() {
            return err("host.workspace: manage_workspaces permission required");
        }
        match sb.service() {
            Some(svc) => workspace_panes(&svc, &workspace_id),
            None => err("host service unavailable"),
        }
    });

    // ── host.agent (async job-id/poll) — gated by the `spawn_agents` grant (§1.10) ──
    const AGENT_DENIED: &str = "host.agent: spawn_agents permission required";
    let sb = sandbox.clone();
    bind!("agentSpawn", move |workspace_id: String,
                              opts: String|
          -> String {
        if !sb.spawn_agents_allowed() {
            return err(AGENT_DENIED);
        }
        match sb.service() {
            Some(svc) => agent_spawn(&svc, sb.plugin_id(), &workspace_id, &opts),
            None => err("host service unavailable"),
        }
    });
    let sb = sandbox.clone();
    bind!("agentResume", move |_workspace_id: String,
                               session_id: String|
          -> String {
        if !sb.spawn_agents_allowed() {
            return err(AGENT_DENIED);
        }
        match sb.service() {
            Some(_) => agent_resume_json(&session_id),
            None => err("host service unavailable"),
        }
    });
    let sb = sandbox.clone();
    bind!("agentGet", move |_workspace_id: String,
                            _key: String|
          -> String {
        if !sb.spawn_agents_allowed() {
            return err(AGENT_DENIED);
        }
        match sb.service() {
            Some(_) => agent_get_json(),
            None => err("host service unavailable"),
        }
    });
    let sb = sandbox.clone();
    bind!("agentSend", move |session_id: String,
                             text: String|
          -> String {
        if !sb.spawn_agents_allowed() {
            return err(AGENT_DENIED);
        }
        match sb.service() {
            Some(svc) => agent_send(&svc, sb.plugin_id(), &session_id, &text),
            None => err("host service unavailable"),
        }
    });
    let sb = sandbox.clone();
    bind!("agentPoll", move |ticket: String| -> String {
        if !sb.spawn_agents_allowed() {
            return err(AGENT_DENIED);
        }
        match sb.service() {
            Some(svc) => agent_poll_json(svc.workers.get(&ticket).as_ref()).to_string(),
            None => err("host service unavailable"),
        }
    });
    let sb = sandbox.clone();
    bind!("agentReap", move |session_id: String| -> String {
        if !sb.spawn_agents_allowed() {
            return err(AGENT_DENIED);
        }
        match sb.service() {
            Some(svc) => agent_reap(&svc, &session_id),
            None => err("host service unavailable"),
        }
    });
    let sb = sandbox.clone();
    bind!("agentUsage", move |_session_id: String| -> String {
        if !sb.spawn_agents_allowed() {
            return err(AGENT_DENIED);
        }
        match sb.service() {
            Some(_) => agent_usage_json(),
            None => err("host service unavailable"),
        }
    });

    // ── host.mem (sync) ──
    let sb = sandbox.clone();
    bind!("memSearch", move |opts: String| -> String {
        match sb.service() {
            Some(svc) => mem_search(&svc, &opts),
            None => err("host service unavailable"),
        }
    });

    Ok(())
}

#[cfg(test)]
mod tests;
