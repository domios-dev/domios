//! Agent spawn & batch command orchestration.

use super::{json_str, resolve_refs, ServiceContext};
use serde::{Deserialize, Serialize};
use tauri::Manager;

// ── Batch ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BatchOp {
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
    pub id: Option<String>,
}

impl ServiceContext {
    /// Execute a batch of commands sequentially. Supports $ref resolution.
    pub async fn batch(&self, commands: Vec<BatchOp>) -> Vec<serde_json::Value> {
        let mut refs: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let mut results: Vec<serde_json::Value> = Vec::new();

        for cmd in &commands {
            let params = resolve_refs(&cmd.params, &refs);

            let result = match cmd.method.as_str() {
                "spawn" => self.batch_spawn(&params).await,
                "send" => self.batch_send(&params),
                "rename" => self.batch_rename(&params),
                "close" => self.batch_close(&params),
                "role" => self.batch_role(&params),
                "notify" => self.batch_notify(&params),
                "can-spawn" => self.batch_can_spawn(&params),
                "report" => self.batch_report(&params),
                other => Err(format!("Unknown method: {}", other)),
            };

            match result {
                Ok(val) => {
                    if let Some(ref id) = cmd.id {
                        if let Some(pid) = val.get("pane_id").and_then(|v| v.as_str()) {
                            refs.insert(id.clone(), pid.to_string());
                        }
                    }
                    results.push(val);
                }
                Err(e) => {
                    log::warn!("[batch] {} failed: {}", cmd.method, e);
                    results.push(serde_json::json!({"error": e, "method": cmd.method}));
                }
            }
        }

        self.emit_state_changed();
        results
    }

    async fn batch_spawn(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let p = SpawnAgentParams::from_json(params)?;
        let result = self.spawn_agent(p, None).await?;
        Ok(serde_json::json!({
            "pane_id": result.pane_id,
            "tab_id": result.tab_id,
            "pty_id": result.pty_id,
            "agent": result.agent,
            "title": result.title,
            "role": result.role,
        }))
    }

    fn batch_send(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let pane_id = json_str(params, "pane_id").ok_or("pane_id required")?;
        let text = json_str(params, "text").unwrap_or_default();
        let enter = params
            .get("enter")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        self.send_text(&pane_id, &text, enter)?;
        Ok(serde_json::json!({"ok": true}))
    }

    fn batch_rename(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let pane_id = json_str(params, "pane_id").ok_or("pane_id required")?;
        let title = json_str(params, "title").ok_or("title required")?;
        self.rename_pane(&pane_id, &title)?;
        Ok(serde_json::json!({"ok": true}))
    }

    fn batch_close(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let pane_id = json_str(params, "pane_id").ok_or("pane_id required")?;
        self.close_pane(&pane_id)?;
        Ok(serde_json::json!({"ok": true}))
    }

    fn batch_role(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let pane_id = json_str(params, "pane_id").ok_or("pane_id required")?;
        let role = json_str(params, "role").ok_or("role required")?;
        let parent = json_str(params, "parent_pane_id");
        let can_spawn = params.get("can_spawn").and_then(|v| v.as_bool());
        self.set_pane_role(&pane_id, &role, parent.as_deref(), can_spawn)?;
        Ok(serde_json::json!({"ok": true}))
    }

    fn batch_notify(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let title = json_str(params, "title").unwrap_or_else(|| "Notification".to_string());
        let body = json_str(params, "body");
        let level = json_str(params, "level");
        let id = self.create_notification(title, body, level.as_deref());
        Ok(serde_json::json!({"ok": true, "id": id}))
    }

    fn batch_can_spawn(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let pane_id = json_str(params, "pane_id").ok_or("pane_id required")?;
        let can_spawn = params
            .get("can_spawn")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        self.set_can_spawn(&pane_id, can_spawn)?;
        Ok(serde_json::json!({"ok": true}))
    }

    fn batch_report(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let pane_id = json_str(params, "pane_id").ok_or("pane_id required")?;
        let message = json_str(params, "message").ok_or("message required")?;
        let details = serde_json::from_value(params.clone()).unwrap_or_default();
        let result = self.submit_report(&pane_id, &message, details)?;
        Ok(serde_json::json!({"ok": true, "report_id": result.report_id}))
    }
}

// ── Agent Spawn ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct SpawnResult {
    pub pane_id: String,
    pub tab_id: String,
    pub pty_id: String,
    pub agent: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub role: Option<String>,
    /// Warning when agent cwd differs from workspace cwd
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SpawnAgentParams {
    pub agent: Option<String>,
    pub agent_name: Option<String>,
    pub args: Option<Vec<String>>,
    pub cwd: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub role: Option<String>,
    pub parent_pane_id: Option<String>,
    pub can_spawn: Option<bool>,
    pub task: Option<String>,
    pub workspace_id: Option<String>,
    /// Force creation as a new tab instead of split pane (even with parent).
    pub force_tab: Option<bool>,
    /// Report target pane ID — for cron agents to report without joining hierarchy.
    pub report_target_pane_id: Option<String>,
    /// Default true; set false to spawn an agent that asks for permission.
    pub skip_permissions: Option<bool>,
    /// CODETERM_PANE_ID of the caller — recorded for report routing fallback.
    pub spawned_by_pane_id: Option<String>,
    /// Provider decides flag syntax via `AgentProvider::model_launch_args`.
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    pub preset: Option<String>,
    pub resume_session_id: Option<String>,
    pub activate: Option<bool>,
    /// Host-set, never from the wire: the fixed structured-prompt server the
    /// host allocated for a `needs_structured_prompt_server` provider. Threaded
    /// into the launch command so the agent binds its SSE bus there.
    #[serde(default, skip)]
    pub structured_prompt_server: Option<crate::agent::provider::StructuredPromptServer>,
}

impl SpawnAgentParams {
    pub fn from_json(params: &serde_json::Value) -> Result<Self, String> {
        Ok(Self {
            agent: json_str(params, "agent"),
            agent_name: json_str(params, "agent_name"),
            args: params.get("args").and_then(|v| v.as_array()).map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            }),
            cwd: json_str(params, "cwd"),
            title: super::spawn_title::normalize_agent_title(json_str(params, "title")),
            description: json_str(params, "description")
                .map(|s| crate::service::normalize_pane_description(&s))
                .transpose()?
                .flatten(),
            role: json_str(params, "role"),
            parent_pane_id: json_str(params, "parent_pane_id")
                .or_else(|| json_str(params, "parent")),
            can_spawn: params.get("can_spawn").and_then(|v| v.as_bool()),
            task: json_str(params, "task"),
            workspace_id: json_str(params, "workspace_id"),
            force_tab: params.get("force_tab").and_then(|v| v.as_bool()),
            report_target_pane_id: json_str(params, "report_target_pane_id"),
            skip_permissions: params.get("skip_permissions").and_then(|v| v.as_bool()),
            spawned_by_pane_id: json_str(params, "spawned_by_pane_id"),
            model: json_str(params, "model"),
            system_prompt: json_str(params, "system_prompt").or_else(|| json_str(params, "system")),
            preset: json_str(params, "preset"),
            resume_session_id: json_str(params, "resume_session_id"),
            activate: params.get("activate").and_then(|v| v.as_bool()),
            structured_prompt_server: None,
        })
    }
}

impl ServiceContext {
    /// Resolve agent cwd against workspace cwd:
    /// - If no cwd specified → inherit from workspace
    /// - If cwd differs from workspace → return warning
    pub(crate) fn resolve_agent_cwd(
        agent_cwd: &Option<String>,
        workspace_cwd: &Option<String>,
    ) -> (Option<String>, Option<String>) {
        match (agent_cwd, workspace_cwd) {
            // No cwd specified — inherit from workspace
            (None, Some(ws_cwd)) => {
                log::info!("[spawn] agent cwd inherited from workspace: {}", ws_cwd);
                (Some(ws_cwd.clone()), None)
            }
            // Both specified and different — warn
            (Some(agent_dir), Some(ws_dir)) if !Self::paths_equal(agent_dir, ws_dir) => {
                let warning = format!(
                    "Agent cwd '{}' differs from workspace cwd '{}'. \
                     The agent will run in '{}', but the workspace is at '{}'.",
                    agent_dir, ws_dir, agent_dir, ws_dir
                );
                log::warn!("[spawn] {}", warning);
                (Some(agent_dir.clone()), Some(warning))
            }
            // cwd specified, matches workspace or no workspace cwd — use as-is
            (Some(dir), _) => (Some(dir.clone()), None),
            // No cwd, no workspace cwd — leave empty
            (None, None) => (None, None),
        }
    }

    /// Compare paths ignoring trailing slashes and case on Windows
    pub(crate) fn paths_equal(a: &str, b: &str) -> bool {
        let norm = |s: &str| {
            let s = s.replace('\\', "/");
            s.trim_end_matches('/').to_string()
        };
        #[cfg(target_os = "windows")]
        {
            norm(a).eq_ignore_ascii_case(&norm(b))
        }
        #[cfg(not(target_os = "windows"))]
        {
            norm(a) == norm(b)
        }
    }

    /// Legacy wrapper for callers still on `SpawnAgentParams` (CLI/REST/batch/cron/socket).
    pub fn spawn_agent_sync(&self, params: SpawnAgentParams) -> Result<SpawnResult, String> {
        self.spawn_agent_sync_with_ready(params, None)
    }

    /// Variant of `spawn_agent_sync` with a readiness oneshot for echo+submit verification.
    pub fn spawn_agent_sync_with_ready(
        &self,
        mut params: SpawnAgentParams,
        ready_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    ) -> Result<SpawnResult, String> {
        params.title = super::spawn_title::normalize_agent_title(params.title.take());
        let role = params.role.clone();
        let agent = params
            .agent
            .clone()
            .unwrap_or_else(crate::config::default_agent_provider);
        let title = params
            .title
            .clone()
            .or_else(|| params.agent_name.clone())
            .unwrap_or_else(|| agent.clone());
        // Reject non-spawnable backends / unknown ids cleanly before building the
        // spec, so the socket-RPC path errors identically to the HTTP path
        // instead of falling through to a generic "Unknown agent provider".
        self.classify_spawn_target(&agent)?;
        let spec = crate::service::spawn::SpawnPaneSpec::from_agent_params_with_chatbackends(
            self.chatbackend_registry.as_ref(),
            params,
        );
        let handle = self.spawn_pane_with_ready(spec, ready_tx)?;
        Ok(SpawnResult {
            pane_id: handle.pane_id,
            tab_id: handle.tab_id,
            pty_id: handle.pty_id.unwrap_or_default(),
            agent,
            title,
            description: handle.description,
            role,
            warning: handle.warning,
        })
    }

    pub async fn spawn_agent(
        &self,
        mut params: SpawnAgentParams,
        ready_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    ) -> Result<SpawnResult, String> {
        params.title = super::spawn_title::normalize_agent_title(params.title.take());
        let default_provider = crate::config::default_agent_provider();
        let agent_cmd = params
            .agent
            .clone()
            .unwrap_or_else(|| default_provider.clone());
        // Classify once: spawnable shell → ChatBackend path, ordinary agent →
        // PTY path, anything else (non-spawnable backend or unknown id) → clean
        // error. A spawnable shell id must never hit "Unknown agent provider".
        let is_shell = self.classify_spawn_target(&agent_cmd)?;
        if is_shell {
            let role = params.role.clone();
            let title = params
                .title
                .clone()
                .or_else(|| params.agent_name.clone())
                .unwrap_or_else(|| agent_cmd.clone());
            let spec = crate::service::spawn::SpawnPaneSpec::from_agent_params_with_chatbackends(
                self.chatbackend_registry.as_ref(),
                params,
            );
            let handle = self.spawn_pane_with_ready(spec, ready_tx)?;
            return Ok(SpawnResult {
                pane_id: handle.pane_id,
                tab_id: handle.tab_id,
                pty_id: handle.pty_id.unwrap_or_default(),
                agent: agent_cmd,
                title,
                description: handle.description,
                role,
                warning: handle.warning,
            });
        }

        let cli_warning =
            crate::service::spawn::provider_cli_install_warning(&self.agent_registry, &agent_cmd)?;
        if let Some(ref warning) = cli_warning {
            log::warn!("[spawn] {}", warning);
        }
        params.model = crate::service::spawn::resolve_fresh_agent_model(
            &self.agent_registry,
            &agent_cmd,
            params.model.take(),
            params.resume_session_id.as_deref(),
        );
        crate::service::spawn::validate_agent_model(
            &self.agent_registry,
            &agent_cmd,
            params.model.as_deref(),
        )?;
        let title_explicit = params
            .title
            .as_deref()
            .map(|t| !t.trim().is_empty())
            .unwrap_or(false)
            || params
                .agent_name
                .as_deref()
                .map(|t| !t.trim().is_empty())
                .unwrap_or(false);
        let title = params
            .title
            .clone()
            .or_else(|| params.agent_name.clone())
            .unwrap_or_else(|| agent_cmd.clone());

        // 1. Find workspace — lock once, resolve everything
        let (ws_id, ws_cwd) = {
            let mux = self.mux.write();
            let from_parent = params
                .parent_pane_id
                .as_ref()
                .and_then(|pid| mux.find_workspace_for_pane(pid));
            let id = from_parent
                .or_else(|| params.workspace_id.clone())
                .or_else(|| mux.snapshot().active_workspace_id.clone())
                .or_else(|| mux.list_workspaces().first().map(|w| w.id.clone()))
                .ok_or("No workspace found")?;
            let cwd = mux.get_workspace(&id).and_then(|ws| ws.cwd.clone());
            (id, cwd)
        };

        // Resolve cwd: inherit from workspace if not specified, warn if different
        let (resolved_cwd, cwd_warning) = Self::resolve_agent_cwd(&params.cwd, &ws_cwd);
        let warning = match (cwd_warning, cli_warning) {
            (Some(a), Some(b)) => Some(format!("{}\n{}", a, b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
        params.cwd = resolved_cwd;

        let default_chat_enabled = self
            .app_handle
            .state::<crate::config::ConfigState>()
            .lock()
            .ui
            .default_new_agent_pane_chat;
        let default_chat_view = crate::service::spawn::should_default_agent_pane_to_chat(
            default_chat_enabled,
            crate::service::spawn::PaneVisibility::Visible,
            self.agent_registry
                .with_provider(&agent_cmd, |p| p.supports_chat_ui())
                .unwrap_or(false),
        );

        // 2. Create new tab in the workspace (each agent gets its own tab)
        let (tab_id, pane_id) = {
            let mut mux = self.mux.write();
            let result = mux.create_tab(&ws_id)?;
            // Pin the sidebar label up-front (rename_pane sync runs later too).
            if let Some(ref t) = params.title {
                if !t.trim().is_empty() {
                    let _ = mux.rename_tab(&ws_id, &result.0, t);
                }
            }
            if let Some(ref dir) = params.cwd {
                if let Some(pane) = mux.get_pane_mut(&result.1) {
                    pane.cwd = Some(dir.clone());
                }
            }
            if params.activate.unwrap_or(false) {
                let _ = mux.set_active_tab(&ws_id, &result.0);
            }
            if default_chat_view {
                if let Some(pane) = mux.get_pane_mut(&result.1) {
                    pane.chat_view = true;
                }
            }
            // Set report target for cron agents
            if let Some(ref rtid) = params.report_target_pane_id {
                if let Some(pane) = mux.get_pane_mut(&result.1) {
                    pane.report_target_pane_id = Some(rtid.clone());
                }
            }
            // Record the spawning agent for report routing fallback, independent of explicit --parent / role hierarchy.
            if let Some(ref sb) = params.spawned_by_pane_id {
                if !sb.is_empty() {
                    if let Some(pane) = mux.get_pane_mut(&result.1) {
                        pane.spawned_by_pane_id = Some(sb.clone());
                    }
                }
            }
            result
        };
        self.emit_state_changed();

        // 3. Set role in hierarchy (before async wait — role is metadata)
        if let Some(ref role_str) = params.role {
            self.set_pane_role(
                &pane_id,
                role_str,
                params.parent_pane_id.as_deref(),
                params.can_spawn,
            )?;
        }

        // 3b. Set auto_approve if spawned with skip_permissions
        if params.skip_permissions.unwrap_or(true) {
            let mut mux = self.mux.write();
            if let Some(pane) = mux.get_pane_mut(&pane_id) {
                pane.auto_approve = true;
            }
        }

        // 4. Rename pane immediately (metadata only). Explicit title → sync the tab label too; otherwise keep the unique auto-generated tab name and
        // only label the pane with the provider id (mirrors the CLI spawn path).
        if title_explicit {
            self.rename_pane(&pane_id, &title)?;
        } else {
            self.set_pane_title(&pane_id, &title)?;
        }

        // Pin agent_type before launch so detect_from_output can start session-id polling.
        // Without this tag, restarts can lose session_id and resume fails silently.
        self.set_and_lock_pane_agent_type(&pane_id, agent_cmd.to_string());

        // 4c. Pin agent_session_id for resume up-front: we know the id from the user's request, so don't wait for detect_session_id polling.
        let launch_session_id = if params.resume_session_id.is_none() {
            self.agent_registry
                .with_provider(&agent_cmd, |p| p.session_id_injection())
                .filter(|mode| !matches!(mode, crate::agent::provider::SessionIdInjection::None))
                .map(|_| uuid::Uuid::new_v4().to_string())
        } else {
            None
        };
        if let Some(ref sid) = params.resume_session_id {
            self.set_agent_session_id(&pane_id, Some(sid.clone()));
        } else if let Some(ref sid) = launch_session_id {
            self.set_agent_session_id(&pane_id, Some(sid.clone()));
        }
        if let Some(model) = params.model.as_ref().filter(|m| !m.trim().is_empty()) {
            if let Some(pane) = self.mux.write().get_pane_mut(&pane_id) {
                pane.agent_session_model = Some(model.trim().to_string());
            }
        }

        // 4d. Structured-prompt server: for providers whose handle is a local
        // server (opencode SSE bus), allocate a fixed loopback port, register
        // the endpoint, and thread it into the launch command so the agent
        // binds its bus there. Capability-gated — never a provider-id branch.
        if self
            .agent_registry
            .with_provider(&agent_cmd, |p| p.needs_structured_prompt_server())
            .unwrap_or(false)
        {
            if let Some(server) = crate::service::structured_prompt::allocate_server() {
                let base_url = format!("http://{}:{}", server.hostname, server.port);
                crate::service::structured_prompt::register_endpoint(
                    &pane_id,
                    base_url,
                    params.cwd.clone().unwrap_or_default(),
                );
                params.structured_prompt_server = Some(server);
            }
        }

        // 5. Launch agent in background thread
        let bg_task = params.task.as_ref().map(|t| match params.role.as_deref() {
            Some("Worker") => format!("{}{}", t, crate::prompts::WORKER_SUFFIX),
            Some("Orchestrator") => format!("{}{}", t, crate::prompts::ORCHESTRATOR_SUFFIX),
            _ => t.clone(),
        });
        // Tag with the spawner's origin prefix uniformly for every provider; delivery handles the marker per provider (TUI-typed for PtyInput, positional after `--` for inline-flag).
        let bg_task = bg_task.map(|t| match self.spawn_task_prefix(&pane_id) {
            Some(pfx) => format!("{}{}", pfx, t),
            None => t,
        });
        let launch_cmd = build_agent_command(
            &agent_cmd,
            &params,
            bg_task.as_deref(),
            launch_session_id.as_deref(),
            &self.agent_registry,
        );
        launch_agent_in_background(
            self.clone(),
            pane_id.clone(),
            tab_id.clone(),
            ws_id.clone(),
            launch_cmd,
            agent_cmd.clone(),
            bg_task,
            ready_tx,
        );

        // Start the structured-prompt SSE subscriber (no-op unless an endpoint
        // was registered above). Reconnect-tolerant — the local server comes up
        // a beat after spawn.
        crate::service::structured_prompt::maybe_start_subscriber(self, &pane_id);

        Ok(SpawnResult {
            pane_id,
            tab_id,
            pty_id: String::new(), // PTY not yet assigned — happens async
            agent: agent_cmd,
            title,
            description: params.description,
            role: params.role,
            warning,
        })
    }

    /// Origin prefix for a spawned agent's first task, derived from the target pane's recorded spawner (spawned_by/parent) — same canonical attribution as cross-pane sends. None for raw spawns (no spawner).
    pub(crate) fn spawn_task_prefix(&self, target_pane: &str) -> Option<String> {
        let mux = self.mux.write();
        let src = mux.get_pane(target_pane).and_then(|p| {
            p.spawned_by_pane_id
                .clone()
                .or_else(|| p.parent_pane_id.clone())
        })?;
        if src.is_empty() {
            return None;
        }
        let label = mux.display_label_for_pane(&src);
        let model = mux
            .get_pane(&src)
            .and_then(|p| p.agent_session_model.clone());
        crate::service::messenger::delivery_prefix(
            true,
            &src,
            target_pane,
            &label,
            model.as_deref(),
            None,
        )
    }
}

pub(crate) use crate::agent::provider::{AgentLaunchParams, ReadinessStrategy, TaskDelivery};
pub(crate) use crate::agent::registry::AgentRegistry;

/// Shared background thread logic for launching agents in a PTY.
/// Uses the AgentRegistry to determine readiness strategy and task delivery.
#[allow(clippy::too_many_arguments)]
pub(crate) fn launch_agent_in_background(
    svc: ServiceContext,
    pane_id: String,
    tab_id: String,
    ws_id: String,
    launch_cmd: String,
    agent_cmd: String,
    task: Option<String>,
    ready_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
) {
    let registry = svc.agent_registry.clone();
    // A booting TUI silently swallows bracket-paste — gate sends (send_text_when_ready) until the background launch clears this. Set BEFORE the
    // thread spawns: callers get the pane id immediately and may send right away.
    if let Some(p) = svc.mux.write().get_pane_mut(&pane_id) {
        p.agent_booting = true;
    }
    std::thread::spawn(move || {
        let set_booting = |value: bool| {
            if let Some(p) = svc.mux.write().get_pane_mut(&pane_id) {
                p.agent_booting = value;
            }
        };
        // 1. Wait for PTY (frontend creates it after mux-state-changed). If frontend doesn't create it (cron/headless tabs), spawn from backend.
        let frontend_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let pty_id = loop {
            if let Some(pid) = svc
                .mux
                .try_write()
                .and_then(|m| m.get_pane(&pane_id).and_then(|p| p.pty_id.clone()))
            {
                break pid;
            }
            if std::time::Instant::now() > frontend_deadline {
                // Frontend didn't create PTY — spawn from backend (cron/headless)
                log::info!("launch_agent: backend PTY spawn for {}", pane_id);
                let cwd = svc
                    .mux
                    .write()
                    .get_pane(&pane_id)
                    .and_then(|p| p.cwd.clone());
                let mut env_vars: std::collections::HashMap<String, String> =
                    std::collections::HashMap::new();
                env_vars.insert("CODETERM_PANE_ID".into(), pane_id.clone());
                env_vars.insert(
                    "CODETERM_SESSION_BINDING_NONCE".into(),
                    svc.ensure_session_binding_nonce(&pane_id),
                );
                env_vars.insert(
                    "CODETERM_SOCKET_PATH".into(),
                    crate::socket::server::socket_display_path(),
                );
                let api_cfg = crate::config::load_config().api;
                let host = if api_cfg.host == "0.0.0.0" {
                    "127.0.0.1".to_string()
                } else {
                    api_cfg.host.clone()
                };
                env_vars.insert(
                    "CODETERM_API_URL".into(),
                    format!("http://{}:{}", host, api_cfg.port),
                );
                if let Some(ref tok) = api_cfg.token {
                    env_vars.insert("CODETERM_API_TOKEN".into(), tok.clone());
                }
                match svc.pty.lock().spawn(None, cwd, 120, 40, env_vars) {
                    Ok((pid, _)) => {
                        if let Some(orphan) = svc.mux.write().link_pty(&pane_id, pid.clone()) {
                            svc.kill_ptys(&[orphan]);
                        }
                        // Start backend reader
                        let svc_arc = std::sync::Arc::new(svc.clone());
                        let pid2 = pid.clone();
                        std::thread::spawn(move || {
                            let rt = tokio::runtime::Builder::new_current_thread()
                                .enable_all()
                                .build()
                                .expect("backend reader runtime");
                            rt.block_on(async {
                                if let Ok(h) = crate::service::pty_lifecycle::start_backend_reader(
                                    &svc_arc, &pid2, true,
                                ) {
                                    let _ = h.await;
                                }
                            });
                        });
                        break pid;
                    }
                    Err(e) => {
                        log::error!(
                            "launch_agent: backend PTY spawn failed for pane {}: {}",
                            pane_id,
                            e
                        );
                        // Clean up the orphaned pane — it has no PTY and is unreachable
                        let _ = svc.close_tab(&ws_id, &tab_id);
                        set_booting(false);
                        if let Some(tx) = ready_tx {
                            let _ = tx.send(Err(format!("backend PTY spawn failed: {}", e)));
                        }
                        return;
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        };

        // 2. Wait for readiness using provider strategy
        let (readiness, delivery) = registry
            .with_provider(&agent_cmd, |p| (p.readiness_strategy(), p.task_delivery()))
            .unwrap_or((
                ReadinessStrategy::FixedDelay {
                    duration: std::time::Duration::from_secs(2),
                },
                TaskDelivery::PtyInput,
            ));

        match readiness {
            ReadinessStrategy::PollShellPrompt { timeout } => {
                crate::service::pty_io::poll_shell_prompt(&svc.screen_state, &pty_id, timeout);
            }
            ReadinessStrategy::FixedDelay { duration } => {
                std::thread::sleep(duration);
            }
        }

        // 3. Send launch command (verified). Shell prompt was already polled in step 2, so require_prompt_first=false avoids a redundant wait.
        let launch_opts = crate::service::inject_verify::VerifyOpts {
            require_prompt_first: false,
            verify_echo: true,
            verify_submit: true,
            prompt_timeout: std::time::Duration::from_secs(0),
            echo_timeout: std::time::Duration::from_secs(3),
            submit_timeout: std::time::Duration::from_secs(15),
        };
        // Use the provider's submit strategy: TUI providers (claude/codex/opencode) hand the shell off to a TUI on launch, so a "new shell prompt"
        // never appears — TuiInputDrained is the right signal there.
        let launch_strategy = registry
            .with_provider(&agent_cmd, |p| p.verify_submit_strategy())
            .unwrap_or(crate::agent::provider::VerifySubmitStrategy::ShellPrompt {
                timeout: std::time::Duration::from_secs(15),
            });
        // Use shell dispatch (bash script on Unix, bracketed paste on Windows) so bash 3.2 hosts (macOS Intel system shell) never see raw ESC markers
        // (#93 — keeps shell-level launch off the bracketed-paste path).
        if let Err(e) =
            svc.send_shell_spawn_verified(&pty_id, &launch_cmd, launch_strategy, launch_opts)
        {
            log::error!(
                "launch_agent: launch_cmd failed for pane {} (pty {}): {}",
                pane_id,
                pty_id,
                e
            );
            set_booting(false);
            if let Some(tx) = ready_tx {
                let _ = tx.send(Err(format!("launch_cmd failed: {}", e)));
            }
            return;
        }

        // 4. For PtyInput delivery, send task immediately — verified send already gated on receiver-side completion of the launch command.
        let is_pty_input = matches!(delivery, TaskDelivery::PtyInput);
        if is_pty_input {
            if let Some(ref task_text) = task {
                let warmup = registry
                    .with_provider(&agent_cmd, |p| p.task_input_warmup())
                    .unwrap_or_default();
                if !warmup.is_zero() {
                    std::thread::sleep(warmup);
                }
                let provider_submit_strategy = registry
                    .with_provider(&agent_cmd, |p| p.verify_submit_strategy())
                    .unwrap_or(crate::agent::provider::VerifySubmitStrategy::ShellPrompt {
                        timeout: std::time::Duration::from_secs(5),
                    });
                let task_opts = crate::service::inject_verify::VerifyOpts {
                    require_prompt_first: false,
                    verify_echo: true,
                    verify_submit: true,
                    prompt_timeout: std::time::Duration::from_secs(0),
                    echo_timeout: std::time::Duration::from_secs(3),
                    submit_timeout: std::time::Duration::from_secs(15),
                };
                if let Err(e) = svc.send_text_verified(
                    &pty_id,
                    task_text,
                    true,
                    provider_submit_strategy,
                    task_opts,
                ) {
                    log::error!(
                        "launch_agent: task delivery failed for pane {} (pty {}): {}",
                        pane_id,
                        pty_id,
                        e
                    );
                    set_booting(false);
                    if let Some(tx) = ready_tx {
                        let _ = tx.send(Err(format!("task delivery failed: {}", e)));
                    }
                    return;
                }
            }
        }

        set_booting(false);
        log::info!("launch_agent: agent ready in pane {}", pane_id);
        if let Some(tx) = ready_tx {
            let _ = tx.send(Ok(()));
        }
    });
}

/// Build a single shell command that launches the agent.
/// Uses the AgentRegistry for known providers; falls back to simple command for unknown agents.
pub(crate) fn build_agent_command(
    agent_cmd: &str,
    params: &SpawnAgentParams,
    task: Option<&str>,
    injected_session_id: Option<&str>,
    registry: &AgentRegistry,
) -> String {
    if let Some(ref sid) = params.resume_session_id {
        let skip = params.skip_permissions.unwrap_or(true);
        if let Some(cmd) = registry
            .with_provider(agent_cmd, |p| p.build_resume_command(sid, skip))
            .flatten()
        {
            return cmd;
        }
    }

    let mut args: Vec<String> = params.args.clone().unwrap_or_default();
    if let Some(ref model) = params.model {
        if !model.is_empty() {
            let resolved = crate::agent::model_catalog::resolve_model(agent_cmd, Some(model));
            let model_args = registry
                .with_provider(agent_cmd, |p| p.model_launch_args(&resolved.id))
                .unwrap_or_else(|| vec!["--model".into(), resolved.id]);
            args.extend(model_args);
        }
    }

    let launch_params = AgentLaunchParams {
        agent_name: params.agent_name.clone(),
        task: task.map(|t| t.to_string()),
        args: if args.is_empty() {
            None
        } else {
            Some(args.clone())
        },
        skip_permissions: params.skip_permissions.unwrap_or(true),
        session_id: injected_session_id.map(std::string::ToString::to_string),
        structured_prompt_server: params.structured_prompt_server.clone(),
    };
    let cmd = registry
        .with_provider(agent_cmd, |p| p.build_launch_command(&launch_params))
        .unwrap_or_else(|| {
            let mut parts: Vec<String> = vec![agent_cmd.to_string()];
            parts.extend(args.iter().cloned());
            parts.join(" ")
        });
    log::info!(
        "[spawn] build_agent_command provider={} model={:?} resume={:?} cmd={}",
        agent_cmd,
        params.model,
        params.resume_session_id,
        cmd
    );
    cmd
}

#[cfg(test)]
mod tests;
