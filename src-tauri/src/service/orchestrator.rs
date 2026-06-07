//! Agent spawn & batch command orchestration.

use super::{json_str, resolve_refs, ServiceContext};
use serde::{Deserialize, Serialize};

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
        let result = self.submit_report(&pane_id, &message)?;
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
    pub role: Option<String>,
    /// Warning when agent cwd differs from workspace cwd
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SpawnAgentParams {
    pub agent: Option<String>,
    pub agent_name: Option<String>,
    pub args: Option<Vec<String>>,
    pub cwd: Option<String>,
    pub title: Option<String>,
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
    pub resume_session_id: Option<String>,
    pub activate: Option<bool>,
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
            title: json_str(params, "title"),
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
            resume_session_id: json_str(params, "resume_session_id"),
            activate: params.get("activate").and_then(|v| v.as_bool()),
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
        params: SpawnAgentParams,
        ready_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    ) -> Result<SpawnResult, String> {
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
        let spec = crate::service::spawn::SpawnPaneSpec::from_agent_params(params);
        let handle = self.spawn_pane_with_ready(spec, ready_tx)?;
        Ok(SpawnResult {
            pane_id: handle.pane_id,
            tab_id: handle.tab_id,
            pty_id: handle.pty_id.unwrap_or_default(),
            agent,
            title,
            role,
            warning: handle.warning,
        })
    }

    pub async fn spawn_agent(
        &self,
        mut params: SpawnAgentParams,
        ready_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    ) -> Result<SpawnResult, String> {
        let default_provider = crate::config::default_agent_provider();
        let agent_cmd = params.agent.as_deref().unwrap_or(&default_provider);
        // Validate against the registry — reject unknown provider ids upfront.
        if self
            .agent_registry
            .with_provider(agent_cmd, |_| ())
            .is_none()
        {
            let available = self.agent_registry.provider_ids().join(", ");
            return Err(format!(
                "Unknown agent provider: '{}'. Available: [{}]",
                agent_cmd, available
            ));
        }
        let title = params
            .title
            .as_deref()
            .or(params.agent_name.as_deref())
            .unwrap_or(agent_cmd);

        // 1. Find workspace — lock once, resolve everything
        let (ws_id, ws_cwd) = {
            let mux = self.mux.lock();
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
        let (resolved_cwd, warning) = Self::resolve_agent_cwd(&params.cwd, &ws_cwd);
        params.cwd = resolved_cwd;

        // 2. Create new tab in the workspace (each agent gets its own tab)
        let (tab_id, pane_id) = {
            let mut mux = self.mux.lock();
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
            let mut mux = self.mux.lock();
            if let Some(pane) = mux.get_pane_mut(&pane_id) {
                pane.auto_approve = true;
            }
        }

        // 4. Rename pane immediately (metadata only)
        self.rename_pane(&pane_id, title)?;

        // Pin agent_type before launch so detect_from_output can start session-id polling.
        // Without this tag, restarts can lose session_id and resume fails silently.
        self.set_and_lock_pane_agent_type(&pane_id, agent_cmd.to_string());

        // 4c. Pin agent_session_id for resume up-front: we know the id from the user's request, so don't wait for detect_session_id polling.
        if let Some(ref sid) = params.resume_session_id {
            self.set_agent_session_id(&pane_id, Some(sid.clone()));
        }

        // 5. Launch agent in background thread
        let bg_task = params.task.as_ref().map(|t| match params.role.as_deref() {
            Some("Worker") => format!("{}{}", t, crate::prompts::WORKER_SUFFIX),
            Some("Orchestrator") => format!("{}{}", t, crate::prompts::ORCHESTRATOR_SUFFIX),
            _ => t.clone(),
        });
        let launch_cmd =
            build_agent_command(agent_cmd, &params, bg_task.as_deref(), &self.agent_registry);
        launch_agent_in_background(
            self.clone(),
            pane_id.clone(),
            tab_id.clone(),
            ws_id.clone(),
            launch_cmd,
            agent_cmd.to_string(),
            bg_task,
            ready_tx,
        );

        Ok(SpawnResult {
            pane_id,
            tab_id,
            pty_id: String::new(), // PTY not yet assigned — happens async
            agent: agent_cmd.to_string(),
            title: title.to_string(),
            role: params.role,
            warning,
        })
    }
}

pub(crate) use crate::agent::provider::{AgentLaunchParams, ReadinessStrategy, TaskDelivery};
pub(crate) use crate::agent::registry::AgentRegistry;

/// Shared background thread logic for launching agents in a PTY.
/// Uses the AgentRegistry to determine readiness strategy and task delivery.
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
    if let Some(p) = svc.mux.lock().get_pane_mut(&pane_id) {
        p.agent_booting = true;
    }
    std::thread::spawn(move || {
        let set_booting = |value: bool| {
            if let Some(p) = svc.mux.lock().get_pane_mut(&pane_id) {
                p.agent_booting = value;
            }
        };
        // 1. Wait for PTY (frontend creates it after mux-state-changed). If frontend doesn't create it (cron/headless tabs), spawn from backend.
        let frontend_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let pty_id = loop {
            if let Some(pid) = svc
                .mux
                .try_lock()
                .and_then(|m| m.get_pane(&pane_id).and_then(|p| p.pty_id.clone()))
            {
                break pid;
            }
            if std::time::Instant::now() > frontend_deadline {
                // Frontend didn't create PTY — spawn from backend (cron/headless)
                log::info!("launch_agent: backend PTY spawn for {}", pane_id);
                let cwd = svc
                    .mux
                    .lock()
                    .get_pane(&pane_id)
                    .and_then(|p| p.cwd.clone());
                let mut env_vars: std::collections::HashMap<String, String> =
                    std::collections::HashMap::new();
                env_vars.insert("CODETERM_PANE_ID".into(), pane_id.clone());
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
                        if let Some(orphan) = svc.mux.lock().link_pty(&pane_id, pid.clone()) {
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
                                    &svc_arc, &pid2,
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
            let model_args = registry
                .with_provider(agent_cmd, |p| p.model_launch_args(model))
                .unwrap_or_else(|| vec!["--model".into(), model.clone()]);
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
        headless: false,
        session_id: None,
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
mod tests {
    use super::*;
    use crate::shell::ShellSyntax;

    fn make_params(agent: &str, task: Option<&str>) -> SpawnAgentParams {
        SpawnAgentParams {
            agent: Some(agent.to_string()),
            task: task.map(|t| t.to_string()),
            agent_name: None,
            args: None,
            cwd: None,
            title: None,
            role: None,
            parent_pane_id: None,
            can_spawn: None,
            workspace_id: None,
            force_tab: None,
            report_target_pane_id: None,
            skip_permissions: None,
            spawned_by_pane_id: None,
            model: None,
            resume_session_id: None,
            activate: None,
        }
    }

    fn test_registry() -> AgentRegistry {
        let reg = AgentRegistry::new();
        reg.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));
        reg
    }

    #[test]
    fn test_build_claude_with_task() {
        let reg = test_registry();
        let params = make_params("claude", Some("count files"));
        let cmd = build_agent_command("claude", &params, Some("count files"), &reg);
        assert!(cmd.contains("--dangerously-skip-permissions"));
        assert!(cmd.contains(".ct-task-"), "task via temp file");
        if cfg!(target_os = "windows") {
            assert!(cmd.contains("Remove-Item Env:CLAUDECODE -ErrorAction SilentlyContinue;"));
            assert!(cmd.contains("(Get-Content"));
        } else {
            assert!(cmd.contains("unset CLAUDECODE &&"));
            assert!(cmd.contains("$(cat /tmp/"));
        }
    }

    #[test]
    fn test_build_claude_no_task() {
        let reg = test_registry();
        let params = make_params("claude", None);
        let cmd = build_agent_command("claude", &params, None, &reg);
        if cfg!(target_os = "windows") {
            assert!(cmd.starts_with("Remove-Item Env:CLAUDECODE"));
            assert!(cmd.contains("[System.Text.Encoding]::UTF8"));
            assert!(cmd.ends_with(" claude --dangerously-skip-permissions"));
        } else {
            assert_eq!(
                cmd,
                "unset CLAUDECODE && claude --dangerously-skip-permissions"
            );
        }
    }

    #[test]
    fn test_build_claude_with_agent_name() {
        let reg = test_registry();
        let mut params = make_params("claude", Some("review code"));
        params.agent_name = Some("architect".to_string());
        let cmd = build_agent_command("claude", &params, None, &reg);
        assert!(cmd.contains("--agent 'architect'"));
    }

    #[test]
    fn test_build_other_agent() {
        let reg = test_registry();
        let mut params = make_params("aider", None);
        params.args = Some(vec!["--model".to_string(), "gpt-4".to_string()]);
        let cmd = build_agent_command("aider", &params, None, &reg);
        assert_eq!(cmd, "aider --model gpt-4");
        assert!(!cmd.contains("CLAUDECODE"));
    }

    #[test]
    fn test_shell_quote() {
        assert_eq!(ShellSyntax::quote("hello"), "'hello'");
        assert_eq!(
            ShellSyntax::quote("count *.tsx files"),
            "'count *.tsx files'"
        );
        if cfg!(target_os = "windows") {
            assert_eq!(ShellSyntax::quote("it's"), "'it''s'");
        } else {
            assert_eq!(ShellSyntax::quote("it's"), "'it'\\''s'");
        }
    }

    // ── 4. build_agent_command with special chars ──

    #[test]
    fn test_build_claude_task_with_newlines() {
        let reg = test_registry();
        let params = make_params("claude", Some("line1\nline2\nline3"));
        let cmd = build_agent_command("claude", &params, Some("line1\nline2\nline3"), &reg);
        // Task goes through temp file, so newlines are preserved in the file
        assert!(
            cmd.contains(".ct-task-"),
            "task with newlines still uses temp file"
        );
        assert!(cmd.contains("--dangerously-skip-permissions"));
    }

    #[test]
    fn test_build_claude_task_with_single_quotes() {
        let reg = test_registry();
        let task = "count files that don't match '*.rs'";
        let params = make_params("claude", Some(task));
        let cmd = build_agent_command("claude", &params, Some(task), &reg);
        // Task is written to a temp file, so quotes don't break the command
        assert!(cmd.contains(".ct-task-"));
    }

    #[test]
    fn test_build_claude_task_with_double_quotes() {
        let reg = test_registry();
        let task = r#"find files matching "*.rs""#;
        let params = make_params("claude", Some(task));
        let cmd = build_agent_command("claude", &params, Some(task), &reg);
        assert!(cmd.contains(".ct-task-"));
    }

    #[test]
    fn test_build_claude_task_with_shell_special_chars() {
        let reg = test_registry();
        let task = "echo $HOME && ls | grep foo; rm -rf /tmp/test";
        let params = make_params("claude", Some(task));
        let cmd = build_agent_command("claude", &params, Some(task), &reg);
        // Should not contain raw special chars in the command itself — they're in the temp file
        assert!(cmd.contains(".ct-task-"));
        assert!(
            !cmd.contains("$HOME"),
            "shell vars should be in temp file, not in command"
        );
    }

    #[test]
    fn test_build_claude_task_temp_file_created() {
        let reg = test_registry();
        let task = "test task content";
        let params = make_params("claude", Some(task));
        let cmd = build_agent_command("claude", &params, Some(task), &reg);
        // Extract the temp file path from the command and verify it was written
        if cfg!(not(target_os = "windows")) {
            // Command looks like: ... "$(cat /tmp/.ct-task-XXXXXXXX)"
            let start = cmd.find("/tmp/.ct-task-").unwrap();
            let end = cmd[start..].find(')').unwrap() + start;
            let path = &cmd[start..end];
            let content = std::fs::read_to_string(path).unwrap();
            assert_eq!(content, task, "temp file should contain the task verbatim");
            // Cleanup
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn test_build_claude_with_extra_args() {
        let reg = test_registry();
        let mut params = make_params("claude", None);
        params.args = Some(vec!["--verbose".to_string(), "--no-cache".to_string()]);
        let cmd = build_agent_command("claude", &params, None, &reg);
        assert!(cmd.contains("--verbose"));
        assert!(cmd.contains("--no-cache"));
        assert!(cmd.ends_with("--verbose --no-cache"));
    }

    #[test]
    fn test_build_with_model_uses_provider_specific_flag() {
        // claude → `--model X`, codex/opencode → `-m X`. The CLI flag is provider-agnostic; the right syntax comes from `model_launch_args`.
        let reg = AgentRegistry::new();
        reg.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));
        reg.register_provider(Box::new(crate::plugin::agent_codex::CodexProvider::new()));
        reg.register_provider(Box::new(
            crate::plugin::agent_opencode::OpenCodeProvider::new(),
        ));

        let mut p = make_params("claude", None);
        p.model = Some("opus".into());
        let cmd = build_agent_command("claude", &p, None, &reg);
        assert!(
            cmd.contains("--model opus"),
            "claude expected --model X: {}",
            cmd
        );

        let mut p = make_params("codex", None);
        p.model = Some("gpt-5.4-mini".into());
        let cmd = build_agent_command("codex", &p, None, &reg);
        assert!(
            cmd.contains("-m gpt-5.4-mini"),
            "codex expected -m X: {}",
            cmd
        );

        let mut p = make_params("opencode", None);
        p.model = Some("github-copilot/claude-haiku-4.5".into());
        let cmd = build_agent_command("opencode", &p, None, &reg);
        assert!(
            cmd.contains("-m github-copilot/claude-haiku-4.5"),
            "opencode expected -m X: {}",
            cmd
        );
    }

    #[test]
    fn test_build_with_resume_session_id_uses_provider_resume_form() {
        let reg = AgentRegistry::new();
        reg.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));
        reg.register_provider(Box::new(crate::plugin::agent_codex::CodexProvider::new()));
        reg.register_provider(Box::new(
            crate::plugin::agent_opencode::OpenCodeProvider::new(),
        ));

        let mut p = make_params("claude", None);
        p.resume_session_id = Some("abc-123".into());
        let cmd = build_agent_command("claude", &p, None, &reg);
        assert!(
            cmd.contains("--resume") && cmd.contains("abc-123"),
            "claude resume: {}",
            cmd
        );

        let mut p = make_params("codex", None);
        p.resume_session_id = Some("019df64f-d798-78c3-967a-d5deeb4896d9".into());
        let cmd = build_agent_command("codex", &p, None, &reg);
        assert!(
            cmd.starts_with("codex resume"),
            "codex resume must start `codex resume`: {}",
            cmd
        );
        assert!(
            cmd.contains("019df64f-d798-78c3-967a-d5deeb4896d9"),
            "codex resume must include uuid: {}",
            cmd
        );
    }

    #[test]
    fn test_build_non_claude_ignores_task_in_command() {
        let reg = test_registry();
        // For non-claude agents, build_agent_command does NOT embed the task
        let params = make_params("aider", Some("review this code"));
        let cmd = build_agent_command("aider", &params, Some("review this code"), &reg);
        // Non-claude: task is NOT added to the command (it's sent separately via PTY) The function just puts agent_cmd + args
        assert_eq!(cmd, "aider");
    }
}
