//! Unified pane spawning.
//!
//! One entry point: [`ServiceContext::spawn_pane`]. Everything that creates
//! a pane — user tabs, CLI `agent spawn`, cron triggers, mem extraction
//! workers, dreaming workers, General Agent sub-spawns — builds a
//! [`SpawnPaneSpec`] and calls [`ServiceContext::spawn_pane`].
//!
//! The spec captures **what** should be in the pane (shell vs agent, which
//! provider, which model, which task) and **how** it should appear (visible
//! tab vs hidden worker). Internals handle workspace resolution, cwd
//! resolution, pane creation, PTY spawn, backend reader start, agent launch
//! command construction, and task delivery.
//!
//! Phase 1 of the unification plan: the type surface lands and `spawn_pane`
//! delegates to the existing `spawn_agent_sync` / `spawn_headless` code
//! paths through an adapter. Phase 3 rewrites those paths to live inside
//! this module directly.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::ServiceContext;
use super::orchestrator::{build_agent_command, launch_agent_in_background, SpawnAgentParams};

// ── Types ───────────────────────────────────────────────────────────────

/// Full description of a pane spawn request.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SpawnPaneSpec {
    /// Explicit workspace. `None` falls back to parent's workspace → active
    /// → first workspace.
    #[serde(default)]
    pub workspace_id: Option<String>,

    /// Parent pane id. Used for workspace lookup and role hierarchy.
    #[serde(default)]
    pub parent_pane_id: Option<String>,

    /// Working directory. `None` inherits from the workspace; mismatch with
    /// workspace cwd produces a warning in the returned handle.
    #[serde(default)]
    pub cwd: Option<String>,

    /// Pane tab title. `None` falls back to agent name / provider id.
    #[serde(default)]
    pub title: Option<String>,

    /// Visible tab vs hidden headless pane.
    #[serde(default)]
    pub visibility: PaneVisibility,

    /// What runs inside the pane.
    pub command: PaneCommand,
}

/// How the pane appears to the user.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaneVisibility {
    /// Normal tab in the tabbar. Lazy PTY: the frontend creates the PTY
    /// after `state-changed`; if no frontend is listening within ~5s the
    /// backend spawns it. This is the default.
    #[default]
    Visible,

    /// Headless pane. `pane.hidden=true`, `pane.show_in_tabbar=false`. The
    /// backend spawns the PTY synchronously and attaches a backend reader
    /// immediately — no frontend involvement needed.
    Hidden,
}

/// What to launch inside the pane.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaneCommand {
    /// Plain shell — spawn a PTY with the user's default shell, no agent
    /// launch command sent.
    Shell,

    /// Launch an agent in the pane after the PTY is ready.
    Agent(AgentSpawnSpec),
}

impl Default for PaneCommand {
    fn default() -> Self {
        PaneCommand::Shell
    }
}

/// Agent-specific spawn fields. Used when [`PaneCommand::Agent`] is set.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AgentSpawnSpec {
    /// Agent provider id (`"claude"`, `"codex"`, `"aider"`, etc). Validated
    /// against `AgentRegistry` — unknown providers return an error from
    /// `spawn_pane` before any pane is created.
    pub provider: String,

    /// Model id resolved within the provider (e.g. `"claude-opus-4-6"` or
    /// alias `"opus"`). `None` uses the provider's default. Forwarded to
    /// `AgentProvider::model_launch_args` at launch time.
    #[serde(default)]
    pub model: Option<String>,

    /// Optional initial task prompt. `None` = interactive agent, no task
    /// sent. Wrapped by `task_preamble` if set.
    #[serde(default)]
    pub task: Option<String>,

    /// Display name override (e.g. `.claude/agents/<name>.md` reference).
    /// Forwarded via `AgentLaunchParams.agent_name`.
    #[serde(default)]
    pub agent_name: Option<String>,

    /// Extra CLI args appended to the launch command (provider-specific).
    #[serde(default)]
    pub extra_args: Vec<String>,

    /// Agent hierarchy role: `"Orchestrator" | "Lead" | "Worker" | "SubWorker"`.
    /// `None` leaves the pane unroled (user tab / standalone agent).
    #[serde(default)]
    pub role: Option<String>,

    /// Whether this pane may spawn its own child agents. Only meaningful
    /// when `role` is set.
    #[serde(default)]
    pub can_spawn: bool,

    /// Auto-accept agent permission prompts (`--dangerously-skip-permissions`
    /// for Claude, provider-specific for others). Defaults to `true`.
    #[serde(default = "default_true")]
    pub auto_approve: bool,

    /// Pane id to report to via `codeterm agent report`. Used for cron
    /// agents that must report back to the pane that triggered them
    /// without joining the orchestrator hierarchy.
    #[serde(default)]
    pub report_target_pane_id: Option<String>,

    /// Opaque worker id (set by `GraphWorkerRegistry` / mem workers).
    /// Populates `pane.worker_id` so the worker tracker can find its pane.
    /// Hidden spawns without a worker_id get one generated for them.
    #[serde(default)]
    pub worker_id: Option<String>,

    /// Origin tag recorded in `GraphWorkerInfo.spawned_by` for hidden
    /// spawns — e.g. `"auto:save"`, `"dreaming"`, `"cron:nightly"`. Used
    /// by the worker panel to group related workers. `None` records as
    /// `"spawn_pane"`.
    #[serde(default)]
    pub spawned_by: Option<String>,

    /// Extra environment variables merged into the PTY env (e.g.
    /// `CODETERM_WORKER_ID`, `CODETERM_JOB_ID`). `CODETERM_PANE_ID`,
    /// `CODETERM_SOCKET_PATH`, `CODETERM_API_URL`, and `CODETERM_API_TOKEN`
    /// are always injected regardless.
    #[serde(default)]
    pub extra_env: HashMap<String, String>,

    /// Preamble wrapped around the task (e.g. `WORKER_TASK_PREAMBLE`,
    /// cron completion hints). `None` = task sent verbatim.
    #[serde(default)]
    pub task_preamble: Option<String>,
}

fn default_true() -> bool {
    true
}

/// Result of a successful spawn.
#[derive(Debug, Clone, Serialize)]
pub struct PaneHandle {
    pub pane_id: String,
    pub tab_id: String,
    /// Populated for [`PaneVisibility::Hidden`] (backend spawns PTY
    /// synchronously). `None` for [`PaneVisibility::Visible`] — the PTY is
    /// created lazily by the frontend, so callers must look it up later if
    /// they need it.
    pub pty_id: Option<String>,
    pub workspace_id: String,
    /// Worker id for hidden agent spawns. Callers can look up the
    /// `GraphWorkerInfo` via `svc.workers.get(worker_id)` for status
    /// tracking and cancellation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
    /// Non-fatal warning. Currently populated when `spec.cwd` differs from
    /// the workspace cwd.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

// ── SpawnAgentParams ↔ SpawnPaneSpec adapters ──────────────────────────

impl SpawnPaneSpec {
    /// Build a spec from the legacy `SpawnAgentParams` used by CLI, REST,
    /// batch, and cron callers. Always produces a Visible + Agent spec
    /// (the only thing SpawnAgentParams can represent).
    pub fn from_agent_params(params: SpawnAgentParams) -> Self {
        Self {
            workspace_id: params.workspace_id.clone(),
            parent_pane_id: params.parent_pane_id.clone(),
            cwd: params.cwd.clone(),
            title: params.title.clone(),
            visibility: PaneVisibility::Visible,
            command: PaneCommand::Agent(AgentSpawnSpec {
                provider: params.agent.clone().unwrap_or_default(),
                model: None,
                task: params.task.clone(),
                agent_name: params.agent_name.clone(),
                extra_args: params.args.clone().unwrap_or_default(),
                role: params.role.clone(),
                can_spawn: params.can_spawn.unwrap_or(false),
                auto_approve: params.skip_permissions.unwrap_or(true),
                report_target_pane_id: params.report_target_pane_id.clone(),
                worker_id: None,
                extra_env: Default::default(),
                task_preamble: None,
                spawned_by: None,
            }),
        }
    }

    /// Convert to the legacy `SpawnAgentParams`. Used by
    /// `spawn_visible_agent_impl` internally when it needs to call
    /// `build_agent_command` (which still takes the legacy struct).
    pub(crate) fn to_agent_params(&self) -> Result<SpawnAgentParams, String> {
        let agent = match &self.command {
            PaneCommand::Agent(a) => a,
            PaneCommand::Shell => {
                return Err("to_agent_params called on Shell spec".into());
            }
        };
        let mut args = agent.extra_args.clone();
        if let Some(ref model) = agent.model {
            // Phase 1: no registry access from the spec — model args are
            // resolved provider-side inside the existing spawn path. Pass
            // the model through as a CLI arg placeholder so provider-
            // specific resolvers can pick it up. Phase 3 replaces this
            // with a direct `model_launch_args` call.
            args.push(format!("--model"));
            args.push(model.clone());
        }
        Ok(SpawnAgentParams {
            agent: Some(agent.provider.clone()),
            agent_name: agent.agent_name.clone(),
            args: if args.is_empty() { None } else { Some(args) },
            cwd: self.cwd.clone(),
            title: self.title.clone(),
            role: agent.role.clone(),
            parent_pane_id: self.parent_pane_id.clone(),
            can_spawn: Some(agent.can_spawn),
            task: agent.task.clone(),
            workspace_id: self.workspace_id.clone(),
            force_tab: None,
            report_target_pane_id: agent.report_target_pane_id.clone(),
            skip_permissions: Some(agent.auto_approve),
        })
    }
}

// ── ServiceContext::spawn_pane ──────────────────────────────────────────

impl ServiceContext {
    /// Unified spawn entry point. Construct a [`SpawnPaneSpec`], call this,
    /// get a [`PaneHandle`]. Every feature that creates panes should funnel
    /// through here — do not call `spawn_agent_sync`, `spawn_headless`, or
    /// `mux.create_tab` directly outside of this module.
    ///
    /// **Errors**: unknown agent provider, missing workspace, or PTY spawn
    /// failure (hidden panes only; visible panes fail async inside the
    /// launch thread).
    pub fn spawn_pane(&self, spec: SpawnPaneSpec) -> Result<PaneHandle, String> {
        match (&spec.visibility, &spec.command) {
            (PaneVisibility::Visible, PaneCommand::Agent(_)) => {
                self.spawn_visible_agent_impl(spec)
            }
            (PaneVisibility::Hidden, PaneCommand::Agent(_)) => {
                self.spawn_hidden_agent_impl(spec)
            }
            (PaneVisibility::Visible, PaneCommand::Shell) => self.spawn_visible_shell_impl(spec),
            (PaneVisibility::Hidden, PaneCommand::Shell) => self.spawn_hidden_shell_impl(spec),
        }
    }
}

// ── Canonical visible-agent implementation ─────────────────────────────
// Moved here from orchestrator::spawn_agent_sync. This is now the single
// source of truth for visible-tab agent spawns; orchestrator::spawn_agent_sync
// is a thin wrapper that builds a spec and calls spawn_pane.

impl ServiceContext {
    /// Canonical implementation for [`PaneVisibility::Visible`] +
    /// [`PaneCommand::Agent`]. Creates a tab, sets role/parent/cwd/auto_approve,
    /// renames, emits state-changed, and kicks off the background launch
    /// thread that waits for PTY + readiness then sends the launch command.
    pub(crate) fn spawn_visible_agent_impl(
        &self,
        spec: SpawnPaneSpec,
    ) -> Result<PaneHandle, String> {
        let agent = match &spec.command {
            PaneCommand::Agent(a) => a.clone(),
            _ => return Err("spawn_visible_agent_impl called without Agent command".into()),
        };

        let default_provider = crate::config::default_agent_provider();
        let agent_cmd = if agent.provider.is_empty() {
            default_provider
        } else {
            agent.provider.clone()
        };

        // Validate provider early — before we touch workspaces, PTYs, or the filesystem.
        if self.agent_registry.with_provider(&agent_cmd, |_| ()).is_none() {
            let available = self.agent_registry.provider_ids().join(", ");
            return Err(format!(
                "Unknown agent provider: '{}'. Available: [{}]",
                agent_cmd, available
            ));
        }

        let title = spec
            .title
            .clone()
            .or_else(|| agent.agent_name.clone())
            .unwrap_or_else(|| agent_cmd.clone());

        // 1. Resolve workspace (parent → explicit → active → first)
        let (ws_id, ws_cwd) = {
            let mux = self.mux.lock();
            let from_parent = spec
                .parent_pane_id
                .as_ref()
                .and_then(|pid| mux.find_workspace_for_pane(pid));
            let id = from_parent
                .or_else(|| spec.workspace_id.clone())
                .or_else(|| mux.snapshot().active_workspace_id.clone())
                .or_else(|| mux.list_workspaces().first().map(|w| w.id.clone()))
                .ok_or("No workspace found")?;
            let cwd = mux.get_workspace(&id).and_then(|ws| ws.cwd.clone());
            (id, cwd)
        };

        // 2. Resolve cwd — inherit from workspace or warn on mismatch.
        let (resolved_cwd, warning) = Self::resolve_agent_cwd(&spec.cwd, &ws_cwd);

        // 3. Create tab + apply metadata, all under one mux lock so the
        //    frontend sees a fully configured pane as soon as state_changed fires.
        let (tab_id, pane_id) = {
            let mut mux = self.mux.lock();
            let result = mux.create_tab(&ws_id)?;
            if let Some(ref dir) = resolved_cwd {
                if let Some(pane) = mux.get_pane_mut(&result.1) {
                    pane.cwd = Some(dir.clone());
                }
            }
            // Role + parent + can_spawn — only when role is set
            if let Some(ref role_str) = agent.role {
                let role = Self::parse_role(role_str)?;
                mux.set_pane_role(&result.1, role, spec.parent_pane_id.as_deref())?;
                if let Err(e) = mux.set_can_spawn(&result.1, agent.can_spawn) {
                    log::warn!("[spawn] set_can_spawn failed: {}", e);
                }
            }
            // Report target for cron / background agents
            if let Some(ref rtid) = agent.report_target_pane_id {
                if let Some(pane) = mux.get_pane_mut(&result.1) {
                    pane.report_target_pane_id = Some(rtid.clone());
                }
            }
            // worker_id (e.g. set by mem extraction when going through this path)
            if let Some(ref wid) = agent.worker_id {
                if let Some(pane) = mux.get_pane_mut(&result.1) {
                    pane.worker_id = Some(wid.clone());
                }
            }
            if agent.auto_approve {
                if let Some(pane) = mux.get_pane_mut(&result.1) {
                    pane.auto_approve = true;
                }
            }
            if let Err(e) = mux.rename_pane(&result.1, &title) {
                log::warn!("[spawn] rename_pane failed: {}", e);
            }
            result
        };
        self.emit_state_changed();

        // 4. Build the task, optionally wrapped in a preamble (cron suffix,
        //    worker_task preamble, role-based suffix).
        let role_suffixed_task = agent.task.as_ref().map(|t| {
            let preamble_wrapped = if let Some(ref preamble) = agent.task_preamble {
                format!("{}{}", preamble, t)
            } else {
                t.clone()
            };
            match agent.role.as_deref() {
                Some("Worker") => format!("{}{}", preamble_wrapped, crate::prompts::WORKER_SUFFIX),
                Some("Lead") => format!("{}{}", preamble_wrapped, crate::prompts::LEAD_SUFFIX),
                Some("Orchestrator") => {
                    format!("{}{}", preamble_wrapped, crate::prompts::ORCHESTRATOR_SUFFIX)
                }
                _ => preamble_wrapped,
            }
        });

        // 5. Background launch — build command via registry, spawn thread.
        // build_agent_command takes legacy SpawnAgentParams; convert via adapter.
        let legacy_params = spec.to_agent_params()?;
        let launch_cmd = build_agent_command(
            &agent_cmd,
            &legacy_params,
            role_suffixed_task.as_deref(),
            &self.agent_registry,
        );
        launch_agent_in_background(
            self.clone(),
            pane_id.clone(),
            launch_cmd,
            agent_cmd.clone(),
            role_suffixed_task,
        );

        Ok(PaneHandle {
            pane_id,
            tab_id,
            pty_id: None,
            workspace_id: ws_id,
            worker_id: agent.worker_id.clone(),
            warning,
        })
    }

    /// Canonical implementation for [`PaneVisibility::Visible`] +
    /// [`PaneCommand::Shell`]. Creates a tab, sets cwd if provided,
    /// optionally renames, and returns. PTY creation stays lazy — the
    /// frontend spawns it after the `state-changed` event. This is what
    /// "new tab" UI actions should call.
    pub(crate) fn spawn_visible_shell_impl(
        &self,
        spec: SpawnPaneSpec,
    ) -> Result<PaneHandle, String> {
        // 1. Resolve workspace (parent → explicit → active → first).
        let (ws_id, ws_cwd) = {
            let mux = self.mux.lock();
            let from_parent = spec
                .parent_pane_id
                .as_ref()
                .and_then(|pid| mux.find_workspace_for_pane(pid));
            let id = from_parent
                .or_else(|| spec.workspace_id.clone())
                .or_else(|| mux.snapshot().active_workspace_id.clone())
                .or_else(|| mux.list_workspaces().first().map(|w| w.id.clone()))
                .ok_or("No workspace found")?;
            let cwd = mux.get_workspace(&id).and_then(|ws| ws.cwd.clone());
            (id, cwd)
        };
        let (resolved_cwd, warning) = Self::resolve_agent_cwd(&spec.cwd, &ws_cwd);

        // 2. Create tab, apply cwd + title.
        let (tab_id, pane_id) = {
            let mut mux = self.mux.lock();
            let result = mux.create_tab(&ws_id)?;
            if let Some(ref dir) = resolved_cwd {
                if let Some(pane) = mux.get_pane_mut(&result.1) {
                    pane.cwd = Some(dir.clone());
                }
            }
            if let Some(ref title) = spec.title {
                if let Err(e) = mux.rename_pane(&result.1, title) {
                    log::warn!("[spawn.shell] rename_pane failed: {}", e);
                }
            }
            result
        };
        self.emit_state_changed();

        Ok(PaneHandle {
            pane_id,
            tab_id,
            pty_id: None,
            workspace_id: ws_id,
            worker_id: None,
            warning,
        })
    }

    /// Canonical implementation for [`PaneVisibility::Hidden`] +
    /// [`PaneCommand::Shell`]. Hidden pane + backend PTY + backend reader,
    /// no agent launched. Useful for background shells that need to run
    /// standalone commands (dreaming SQL phases, background maintenance)
    /// without the full agent machinery.
    pub(crate) fn spawn_hidden_shell_impl(
        &self,
        spec: SpawnPaneSpec,
    ) -> Result<PaneHandle, String> {
        use std::collections::HashMap;

        // Workspace resolution (same as hidden+agent).
        let ws_id = {
            let mux = self.mux.lock();
            spec.workspace_id
                .clone()
                .or_else(|| mux.snapshot().active_workspace_id.clone())
                .or_else(|| mux.list_workspaces().first().map(|w| w.id.clone()))
                .ok_or("No workspace for hidden shell")?
        };
        let ws_cwd = {
            let mux = self.mux.lock();
            mux.get_workspace(&ws_id).and_then(|ws| ws.cwd.clone())
        };
        let resolved_cwd = spec.cwd.clone().or(ws_cwd);

        // Create hidden pane.
        let (tab_id, pane_id) = {
            let mut mux = self.mux.lock();
            let result = mux.create_tab(&ws_id)?;
            if let Some(pane) = mux.get_pane_mut(&result.1) {
                pane.hidden = true;
                pane.show_in_tabbar = false;
                if let Some(ref dir) = resolved_cwd {
                    pane.cwd = Some(dir.clone());
                }
            }
            result
        };

        // Backend PTY.
        let mut env_vars: HashMap<String, String> = HashMap::new();
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

        let pty_id = {
            let mut pty = self.pty.lock();
            pty.spawn(None, resolved_cwd, 120, 40, env_vars)
                .map(|(id, _)| id)?
        };
        self.mux.lock().link_pty(&pane_id, pty_id.clone());
        self.emit_state_changed();

        // Backend reader.
        let svc_reader = std::sync::Arc::new(self.clone());
        let pty_reader = pty_id.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("backend reader runtime");
            rt.block_on(async {
                match crate::service::pty_lifecycle::start_backend_reader(&svc_reader, &pty_reader)
                {
                    Ok(handle) => {
                        let _ = handle.await;
                    }
                    Err(e) => log::warn!("[spawn.hidden_shell] backend reader: {}", e),
                }
            });
        });

        Ok(PaneHandle {
            pane_id,
            tab_id,
            pty_id: Some(pty_id),
            workspace_id: ws_id,
            worker_id: None,
            warning: None,
        })
    }

    /// Canonical implementation for [`PaneVisibility::Hidden`] +
    /// [`PaneCommand::Agent`]. Hidden pane from birth, backend PTY spawn,
    /// backend reader, agent launch — all synchronous so the PaneHandle
    /// returns with a real pty_id. Used by mem extraction/dreaming workers.
    pub(crate) fn spawn_hidden_agent_impl(
        &self,
        spec: SpawnPaneSpec,
    ) -> Result<PaneHandle, String> {
        use std::collections::HashMap;

        let agent = match &spec.command {
            PaneCommand::Agent(a) => a.clone(),
            _ => return Err("spawn_hidden_agent_impl called without Agent command".into()),
        };

        if agent.provider.is_empty() {
            return Err("AgentSpawnSpec.provider is required for hidden spawns".into());
        }
        if self
            .agent_registry
            .with_provider(&agent.provider, |_| ())
            .is_none()
        {
            let available = self.agent_registry.provider_ids().join(", ");
            return Err(format!(
                "Unknown agent provider: '{}'. Available: [{}]",
                agent.provider, available
            ));
        }

        // 1. Resolve workspace — explicit id → active → first.
        let ws_id = {
            let mux = self.mux.lock();
            spec.workspace_id
                .clone()
                .or_else(|| mux.snapshot().active_workspace_id.clone())
                .or_else(|| mux.list_workspaces().first().map(|w| w.id.clone()))
                .ok_or("No workspace for hidden spawn")?
        };
        let ws_cwd = {
            let mux = self.mux.lock();
            mux.get_workspace(&ws_id).and_then(|ws| ws.cwd.clone())
        };
        let resolved_cwd = spec.cwd.clone().or(ws_cwd);

        // 2. Create hidden pane in a single mux lock.
        let (tab_id, pane_id) = {
            let mut mux = self.mux.lock();
            let result = mux.create_tab(&ws_id)?;
            if let Some(pane) = mux.get_pane_mut(&result.1) {
                pane.hidden = true;
                pane.show_in_tabbar = false;
                if let Some(ref wid) = agent.worker_id {
                    pane.worker_id = Some(wid.clone());
                }
                if agent.auto_approve {
                    pane.auto_approve = true;
                }
                if let Some(ref dir) = resolved_cwd {
                    pane.cwd = Some(dir.clone());
                }
                if let Some(ref rtid) = agent.report_target_pane_id {
                    pane.report_target_pane_id = Some(rtid.clone());
                }
            }
            result
        };

        // 3. Spawn PTY from backend (no frontend involvement for hidden panes).
        let mut env_vars: HashMap<String, String> = HashMap::new();
        env_vars.insert("CODETERM_PANE_ID".into(), pane_id.clone());
        if let Some(ref wid) = agent.worker_id {
            env_vars.insert("CODETERM_WORKER_ID".into(), wid.clone());
        }
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
        for (k, v) in &agent.extra_env {
            env_vars.insert(k.clone(), v.clone());
        }

        let pty_id = {
            let mut pty = self.pty.lock();
            pty.spawn(None, resolved_cwd, 120, 40, env_vars)
                .map(|(id, _)| id)?
        };

        self.mux.lock().link_pty(&pane_id, pty_id.clone());
        self.emit_state_changed();

        // 4. Resolve model launch args via the provider's catalog.
        let model_args: Vec<String> = agent
            .model
            .as_ref()
            .and_then(|m| {
                self.agent_registry
                    .with_provider(&agent.provider, |p| p.model_launch_args(m))
            })
            .unwrap_or_default();

        // 5. Build the task, optionally wrapped in a preamble.
        let wrapped_task = agent.task.as_ref().map(|t| {
            if let Some(ref preamble) = agent.task_preamble {
                format!("{}{}", preamble, t)
            } else {
                t.clone()
            }
        });

        // 6. Background thread: reader + launch agent.
        let svc_outer = std::sync::Arc::new(self.clone());
        let pane_id_outer = pane_id.clone();
        let pty_id_outer = pty_id.clone();
        let provider = agent.provider.clone();
        let task_for_thread = wrapped_task.clone();
        std::thread::spawn(move || {
            // Reader runtime
            let svc_reader = svc_outer.clone();
            let pty_reader = pty_id_outer.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("backend reader runtime");
                rt.block_on(async {
                    match crate::service::pty_lifecycle::start_backend_reader(
                        &svc_reader,
                        &pty_reader,
                    ) {
                        Ok(handle) => {
                            let _ = handle.await;
                        }
                        Err(e) => log::warn!("[spawn.hidden] backend reader: {}", e),
                    }
                });
            });

            crate::service::pty_io::poll_shell_prompt(
                &svc_outer.screen_state,
                &pty_id_outer,
                std::time::Duration::from_secs(10),
            );

            let launch_params = crate::agent::provider::AgentLaunchParams {
                agent_name: None,
                task: task_for_thread,
                args: if model_args.is_empty() {
                    None
                } else {
                    Some(model_args)
                },
                skip_permissions: true,
            };
            let launch_cmd = svc_outer
                .agent_registry
                .with_provider(&provider, |p| p.build_launch_command(&launch_params))
                .unwrap_or_else(|| format!("{} --dangerously-skip-permissions", provider));

            if let Err(e) = svc_outer.send_text(&pane_id_outer, &launch_cmd, true) {
                log::error!("[spawn.hidden] send launch cmd: {}", e);
            }
            log::debug!(
                "[spawn.hidden] agent launched pane={} provider={}",
                pane_id_outer,
                provider
            );
        });

        Ok(PaneHandle {
            pane_id,
            tab_id,
            pty_id: Some(pty_id),
            workspace_id: ws_id,
            worker_id: agent.worker_id.clone(),
            warning: None,
        })
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn agent_spec(provider: &str) -> SpawnPaneSpec {
        SpawnPaneSpec {
            visibility: PaneVisibility::Visible,
            command: PaneCommand::Agent(AgentSpawnSpec {
                provider: provider.into(),
                auto_approve: true,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn default_visibility_is_visible() {
        let spec = SpawnPaneSpec::default();
        assert_eq!(spec.visibility, PaneVisibility::Visible);
    }

    #[test]
    fn default_command_is_shell() {
        let spec = SpawnPaneSpec::default();
        assert!(matches!(spec.command, PaneCommand::Shell));
    }

    #[test]
    fn agent_auto_approve_defaults_true_via_serde() {
        let json = r#"{"provider": "claude"}"#;
        let agent: AgentSpawnSpec = serde_json::from_str(json).unwrap();
        assert!(agent.auto_approve);
    }

    #[test]
    fn agent_auto_approve_defaults_true_via_struct() {
        let agent = AgentSpawnSpec {
            provider: "claude".into(),
            auto_approve: default_true(),
            ..Default::default()
        };
        assert!(agent.auto_approve);
    }

    #[test]
    fn shell_spec_roundtrips_json() {
        let spec = SpawnPaneSpec {
            title: Some("scratch".into()),
            ..Default::default()
        };
        let json = serde_json::to_string(&spec).unwrap();
        let back: SpawnPaneSpec = serde_json::from_str(&json).unwrap();
        assert!(matches!(back.command, PaneCommand::Shell));
        assert_eq!(back.title.as_deref(), Some("scratch"));
    }

    #[test]
    fn agent_spec_roundtrips_json() {
        let spec = SpawnPaneSpec {
            visibility: PaneVisibility::Hidden,
            command: PaneCommand::Agent(AgentSpawnSpec {
                provider: "claude".into(),
                model: Some("opus".into()),
                task: Some("hello".into()),
                role: Some("Worker".into()),
                can_spawn: false,
                auto_approve: true,
                worker_id: Some("w-123".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let json = serde_json::to_string(&spec).unwrap();
        let back: SpawnPaneSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(back.visibility, PaneVisibility::Hidden);
        match back.command {
            PaneCommand::Agent(a) => {
                assert_eq!(a.provider, "claude");
                assert_eq!(a.model.as_deref(), Some("opus"));
                assert_eq!(a.task.as_deref(), Some("hello"));
                assert_eq!(a.role.as_deref(), Some("Worker"));
                assert_eq!(a.worker_id.as_deref(), Some("w-123"));
                assert!(a.auto_approve);
            }
            _ => panic!("expected Agent"),
        }
    }

    #[test]
    fn to_agent_params_carries_model_through_args() {
        let mut spec = agent_spec("claude");
        if let PaneCommand::Agent(ref mut a) = spec.command {
            a.model = Some("opus".into());
            a.task = Some("count files".into());
        }
        let params = spec.to_agent_params().unwrap();
        assert_eq!(params.agent.as_deref(), Some("claude"));
        assert_eq!(params.task.as_deref(), Some("count files"));
        let args = params.args.unwrap();
        assert!(args.iter().any(|a| a == "--model"));
        assert!(args.iter().any(|a| a == "opus"));
    }

    #[test]
    fn to_agent_params_passes_role_and_report_target() {
        let mut spec = agent_spec("claude");
        spec.parent_pane_id = Some("parent-1".into());
        if let PaneCommand::Agent(ref mut a) = spec.command {
            a.role = Some("Lead".into());
            a.can_spawn = true;
            a.report_target_pane_id = Some("cron-host".into());
        }
        let params = spec.to_agent_params().unwrap();
        assert_eq!(params.role.as_deref(), Some("Lead"));
        assert_eq!(params.parent_pane_id.as_deref(), Some("parent-1"));
        assert_eq!(params.can_spawn, Some(true));
        assert_eq!(params.report_target_pane_id.as_deref(), Some("cron-host"));
    }

    #[test]
    fn to_agent_params_fails_on_shell_spec() {
        // to_agent_params is a legacy adapter — it's only meaningful for
        // agent spawns. Shell spawns use the direct implementation paths
        // in spawn_visible_shell_impl / spawn_hidden_shell_impl and never
        // hit the SpawnAgentParams conversion.
        let spec = SpawnPaneSpec::default();
        let err = spec.to_agent_params().unwrap_err();
        assert!(err.contains("Shell"));
    }
}
