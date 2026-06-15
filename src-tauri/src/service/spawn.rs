use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tauri::Manager;

use super::orchestrator::{build_agent_command, launch_agent_in_background, SpawnAgentParams};
use super::ServiceContext;
use crate::agent::registry::AgentRegistry;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SpawnPaneSpec {
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub parent_pane_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub visibility: PaneVisibility,
    #[serde(default)]
    pub eager: bool,
    pub command: PaneCommand,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaneVisibility {
    #[default]
    Visible,
    Hidden,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaneCommand {
    Shell,
    Agent(AgentSpawnSpec),
}

impl Default for PaneCommand {
    fn default() -> Self {
        PaneCommand::Shell
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AgentSpawnSpec {
    pub provider: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub task: Option<String>,
    #[serde(default)]
    pub agent_name: Option<String>,
    #[serde(default)]
    pub extra_args: Vec<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub can_spawn: Option<bool>,
    #[serde(default = "default_true")]
    pub auto_approve: bool,
    #[serde(default)]
    pub report_target_pane_id: Option<String>,
    #[serde(default)]
    pub worker_id: Option<String>,
    #[serde(default)]
    pub spawned_by: Option<String>,
    #[serde(default)]
    pub extra_env: HashMap<String, String>,
    #[serde(default)]
    pub task_preamble: Option<String>,
    #[serde(default)]
    pub spawned_by_pane_id: Option<String>,
    #[serde(default)]
    pub resume_session_id: Option<String>,
    #[serde(default)]
    pub activate: Option<bool>,
}

fn default_true() -> bool {
    true
}

fn valid_cli_binary(binary: &str) -> bool {
    !binary.is_empty()
        && !binary.chars().any(|c| {
            matches!(
                c,
                '/' | '\\' | ' ' | ';' | '|' | '&' | '$' | '`' | '<' | '>' | '\n'
            )
        })
}

pub(crate) fn cli_binary_on_path(binary: &str) -> bool {
    if !valid_cli_binary(binary) {
        return false;
    }
    let cmd_name = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };
    let mut cmd = std::process::Command::new(cmd_name);
    cmd.arg(binary);
    crate::process_util::no_window_std(&mut cmd);
    matches!(cmd.output(), Ok(out) if out.status.success() && !out.stdout.is_empty())
}

pub(crate) fn ensure_provider_cli_available(
    registry: &AgentRegistry,
    provider_id: &str,
) -> Result<(), String> {
    let Some((display_name, cli_binary, install_command)) =
        registry.with_provider(provider_id, |p| {
            (
                p.display_name().to_string(),
                p.cli_binary().map(str::to_string),
                p.install_command(crate::agent::provider::current_platform_tag()),
            )
        })
    else {
        let available = registry.provider_ids().join(", ");
        return Err(format!(
            "Unknown agent provider: '{}'. Available: [{}]",
            provider_id, available
        ));
    };
    let Some(binary) = cli_binary else {
        return Ok(());
    };
    if !valid_cli_binary(&binary) {
        return Err(format!(
            "Provider '{}' declares invalid CLI binary '{}'",
            provider_id, binary
        ));
    }
    if cli_binary_on_path(&binary) {
        return Ok(());
    }
    let install_hint = install_command
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "No install command is available for this platform.".to_string());
    Err(format!(
        "Provider not installed: {} requires CLI `{}` on PATH. Install command: {}",
        display_name, binary, install_hint
    ))
}

pub(crate) fn should_default_agent_pane_to_chat(
    default_enabled: bool,
    visibility: PaneVisibility,
    provider_supports_chat_ui: bool,
) -> bool {
    default_enabled && visibility == PaneVisibility::Visible && provider_supports_chat_ui
}

#[derive(Debug, Clone, Serialize)]
pub struct PaneHandle {
    pub pane_id: String,
    pub tab_id: String,
    pub pty_id: Option<String>,
    pub workspace_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

impl SpawnPaneSpec {
    pub fn from_agent_params(params: SpawnAgentParams) -> Self {
        Self {
            workspace_id: params.workspace_id.clone(),
            parent_pane_id: params.parent_pane_id.clone(),
            cwd: params.cwd.clone(),
            title: params.title.clone(),
            visibility: PaneVisibility::Visible,
            eager: false,
            command: PaneCommand::Agent(AgentSpawnSpec {
                provider: params.agent.clone().unwrap_or_default(),
                model: params.model.clone(),
                task: params.task.clone(),
                agent_name: params.agent_name.clone(),
                extra_args: params.args.clone().unwrap_or_default(),
                role: params.role.clone(),
                can_spawn: params.can_spawn,
                auto_approve: params.skip_permissions.unwrap_or(true),
                report_target_pane_id: params.report_target_pane_id.clone(),
                worker_id: None,
                extra_env: Default::default(),
                task_preamble: None,
                spawned_by: None,
                spawned_by_pane_id: params.spawned_by_pane_id.clone(),
                resume_session_id: params.resume_session_id.clone(),
                activate: params.activate,
            }),
        }
    }

    pub(crate) fn to_agent_params(&self) -> Result<SpawnAgentParams, String> {
        let agent = match &self.command {
            PaneCommand::Agent(a) => a,
            PaneCommand::Shell => {
                return Err("to_agent_params called on Shell spec".into());
            }
        };
        let args = agent.extra_args.clone();
        Ok(SpawnAgentParams {
            agent: Some(agent.provider.clone()),
            agent_name: agent.agent_name.clone(),
            args: if args.is_empty() { None } else { Some(args) },
            cwd: self.cwd.clone(),
            title: self.title.clone(),
            role: agent.role.clone(),
            parent_pane_id: self.parent_pane_id.clone(),
            can_spawn: agent.can_spawn,
            task: agent.task.clone(),
            workspace_id: self.workspace_id.clone(),
            force_tab: None,
            report_target_pane_id: agent.report_target_pane_id.clone(),
            skip_permissions: Some(agent.auto_approve),
            spawned_by_pane_id: agent.spawned_by_pane_id.clone(),
            model: agent.model.clone(),
            resume_session_id: agent.resume_session_id.clone(),
            activate: agent.activate,
        })
    }
}

impl ServiceContext {
    pub fn spawn_pane(&self, spec: SpawnPaneSpec) -> Result<PaneHandle, String> {
        self.spawn_pane_with_ready(spec, None)
    }

    pub fn spawn_pane_with_ready(
        &self,
        spec: SpawnPaneSpec,
        ready_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    ) -> Result<PaneHandle, String> {
        match (&spec.visibility, &spec.command) {
            (PaneVisibility::Visible, PaneCommand::Agent(_)) => {
                self.spawn_visible_agent_impl(spec, ready_tx)
            }
            (PaneVisibility::Hidden, PaneCommand::Agent(_)) => self.spawn_hidden_agent_impl(spec),
            (PaneVisibility::Visible, PaneCommand::Shell) => self.spawn_visible_shell_impl(spec),
            (PaneVisibility::Hidden, PaneCommand::Shell) => self.spawn_hidden_shell_impl(spec),
        }
    }
}

pub(crate) fn validate_agent_model(
    registry: &AgentRegistry,
    provider_id: &str,
    model: Option<&str>,
) -> Result<(), String> {
    let Some(model) = model.map(str::trim).filter(|m| !m.is_empty()) else {
        return Ok(());
    };
    let models = registry
        .with_provider(provider_id, |p| p.available_models())
        .unwrap_or_default();
    if models.is_empty() {
        return Ok(());
    }
    if models.iter().any(|m| m.id == model) {
        return Ok(());
    }

    let suffix_matches: Vec<&str> = models
        .iter()
        .filter_map(|m| {
            m.id.rsplit_once('/')
                .map(|(_, suffix)| (m.id.as_str(), suffix))
        })
        .filter_map(|(id, suffix)| (suffix == model).then_some(id))
        .collect();
    let suggestion = if suffix_matches.len() == 1 {
        format!(" Did you mean '{}'?", suffix_matches[0])
    } else {
        String::new()
    };
    Err(format!(
        "Invalid model '{}' for provider '{}'.{} Run `codeterm agent models {}` to see available models.",
        model, provider_id, suggestion, provider_id
    ))
}

pub(crate) fn resolve_fresh_agent_model(
    registry: &AgentRegistry,
    provider_id: &str,
    requested: Option<String>,
    resume_session_id: Option<&str>,
) -> Option<String> {
    if resume_session_id.is_some() {
        return requested.and_then(non_empty_model);
    }
    if let Some(model) = requested.and_then(non_empty_model) {
        return Some(model);
    }
    if let Some(model) = crate::config::provider_config(provider_id)
        .default_model
        .and_then(non_empty_model)
    {
        if model_is_advertised(registry, provider_id, &model) {
            return Some(model);
        }
        log::warn!(
            "[spawn] ignoring stale default model '{}' for provider '{}'",
            model,
            provider_id
        );
    }
    registry
        .with_provider(provider_id, |p| p.default_model())
        .flatten()
        .and_then(non_empty_model)
}

fn non_empty_model(model: String) -> Option<String> {
    let trimmed = model.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn model_is_advertised(registry: &AgentRegistry, provider_id: &str, model: &str) -> bool {
    registry
        .with_provider(provider_id, |p| p.available_models())
        .map(|models| models.is_empty() || models.iter().any(|m| m.id == model))
        .unwrap_or(false)
}

impl ServiceContext {
    pub(crate) fn spawn_visible_agent_impl(
        &self,
        spec: SpawnPaneSpec,
        ready_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    ) -> Result<PaneHandle, String> {
        let agent = match &spec.command {
            PaneCommand::Agent(a) => a.clone(),
            _ => return Err("spawn_visible_agent_impl called without Agent command".into()),
        };

        if let Some(ref role_str) = agent.role {
            ServiceContext::parse_role(role_str)?;
        }

        let default_provider = crate::config::default_agent_provider();
        let agent_cmd = if agent.provider.is_empty() {
            default_provider
        } else {
            agent.provider.clone()
        };

        if self
            .agent_registry
            .with_provider(&agent_cmd, |_| ())
            .is_none()
        {
            let available = self.agent_registry.provider_ids().join(", ");
            return Err(format!(
                "Unknown agent provider: '{}'. Available: [{}]",
                agent_cmd, available
            ));
        }
        ensure_provider_cli_available(&self.agent_registry, &agent_cmd)?;
        let launch_model = resolve_fresh_agent_model(
            &self.agent_registry,
            &agent_cmd,
            agent.model.clone(),
            agent.resume_session_id.as_deref(),
        );
        validate_agent_model(&self.agent_registry, &agent_cmd, launch_model.as_deref())?;

        let title = spec
            .title
            .clone()
            .or_else(|| agent.agent_name.clone())
            .unwrap_or_else(|| agent_cmd.clone());

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

        let default_chat_enabled = self
            .app_handle
            .state::<crate::config::ConfigState>()
            .lock()
            .ui
            .default_new_agent_pane_chat;
        let default_chat_view = should_default_agent_pane_to_chat(
            default_chat_enabled,
            spec.visibility,
            self.agent_registry
                .with_provider(&agent_cmd, |p| p.supports_chat_ui())
                .unwrap_or(false),
        );

        let (tab_id, pane_id) = {
            let mut mux = self.mux.lock();
            let result = mux.create_tab(&ws_id)?;
            // Apply explicit --title to tab.title up-front (mirrors REST path).
            if let Some(ref t) = spec.title {
                if !t.trim().is_empty() {
                    let _ = mux.rename_tab(&ws_id, &result.0, t);
                }
            }
            if let Some(ref dir) = resolved_cwd {
                if let Some(pane) = mux.get_pane_mut(&result.1) {
                    pane.cwd = Some(dir.clone());
                }
            }
            if let Some(ref rtid) = agent.report_target_pane_id {
                if let Some(pane) = mux.get_pane_mut(&result.1) {
                    pane.report_target_pane_id = Some(rtid.clone());
                }
            }
            if let Some(ref sb) = agent.spawned_by_pane_id {
                if !sb.is_empty() {
                    if let Some(pane) = mux.get_pane_mut(&result.1) {
                        pane.spawned_by_pane_id = Some(sb.clone());
                    }
                }
            }
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
            if default_chat_view {
                if let Some(pane) = mux.get_pane_mut(&result.1) {
                    pane.chat_view = true;
                }
            }
            if let Err(e) = mux.rename_pane(&result.1, &title) {
                log::warn!("[spawn] rename_pane failed: {}", e);
            }
            if agent.activate.unwrap_or(false) {
                let _ = mux.set_active_tab(&ws_id, &result.0);
            }
            result
        };

        // Apply role + can_spawn after the tab is created. Done outside the create-tab lock so set_pane_role's full validation runs and emits state
        // changes; `can_spawn` is only forced when the caller explicitly passed --can-spawn (Some), preserving role defaults (Orchestrator defaults true, Worker false).
        if let Some(ref role_str) = agent.role {
            if let Err(e) = self.set_pane_role(
                &pane_id,
                role_str,
                spec.parent_pane_id.as_deref(),
                agent.can_spawn,
            ) {
                log::warn!(
                    "[spawn] set_pane_role({}, {}) failed: {}",
                    pane_id,
                    role_str,
                    e
                );
            }
        }
        // Pin agent_type before launch so detect_from_output has a durable session-id source.
        // Missing this tag leaves Codex/OpenCode panes unresumable after restart.
        self.set_and_lock_pane_agent_type(&pane_id, agent_cmd.clone());
        let launch_session_id = if agent.resume_session_id.is_none() {
            self.agent_registry
                .with_provider(&agent_cmd, |p| p.session_id_injection())
                .filter(|mode| !matches!(mode, crate::agent::provider::SessionIdInjection::None))
                .map(|_| uuid::Uuid::new_v4().to_string())
        } else {
            None
        };
        if let Some(ref sid) = agent.resume_session_id {
            self.set_agent_session_id(&pane_id, Some(sid.clone()));
        } else if let Some(ref sid) = launch_session_id {
            self.set_agent_session_id(&pane_id, Some(sid.clone()));
        }
        if let Some(ref model) = launch_model {
            if let Some(pane) = self.mux.lock().get_pane_mut(&pane_id) {
                pane.agent_session_model = Some(model.clone());
            }
        }
        self.emit_state_changed();

        let role_suffixed_task = agent.task.as_ref().map(|t| {
            let preamble_wrapped = if let Some(ref preamble) = agent.task_preamble {
                format!("{}{}", preamble, t)
            } else {
                t.clone()
            };
            match agent.role.as_deref() {
                Some("Worker") => format!("{}{}", preamble_wrapped, crate::prompts::WORKER_SUFFIX),
                Some("Orchestrator") => {
                    format!(
                        "{}{}",
                        preamble_wrapped,
                        crate::prompts::ORCHESTRATOR_SUFFIX
                    )
                }
                _ => preamble_wrapped,
            }
        });

        let mut legacy_params = spec.to_agent_params()?;
        legacy_params.model = launch_model;
        let launch_cmd = build_agent_command(
            &agent_cmd,
            &legacy_params,
            role_suffixed_task.as_deref(),
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
            role_suffixed_task,
            ready_tx,
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

    pub(crate) fn spawn_visible_shell_impl(
        &self,
        spec: SpawnPaneSpec,
    ) -> Result<PaneHandle, String> {
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
                let _ = mux.rename_tab(&ws_id, &result.0, title);
            }
            result
        };
        self.emit_state_changed();

        let pty_id = if spec.eager {
            self.ensure_pane_pty_for_send(&pane_id)?;
            self.mux
                .lock()
                .get_pane(&pane_id)
                .and_then(|p| p.pty_id.clone())
        } else {
            None
        };

        Ok(PaneHandle {
            pane_id,
            tab_id,
            pty_id,
            workspace_id: ws_id,
            worker_id: None,
            warning,
        })
    }

    pub(crate) fn spawn_hidden_shell_impl(
        &self,
        spec: SpawnPaneSpec,
    ) -> Result<PaneHandle, String> {
        use std::collections::HashMap;

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
        if let Some(orphan) = self.mux.lock().link_pty(&pane_id, pty_id.clone()) {
            self.kill_ptys(&[orphan]);
        }
        self.emit_state_changed();

        let svc_reader = std::sync::Arc::new(self.clone());
        let pty_reader = pty_id.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("backend reader runtime");
            rt.block_on(async {
                match crate::service::pty_lifecycle::start_backend_reader(
                    &svc_reader,
                    &pty_reader,
                    true,
                ) {
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
        ensure_provider_cli_available(&self.agent_registry, &agent.provider)?;
        let launch_model = resolve_fresh_agent_model(
            &self.agent_registry,
            &agent.provider,
            agent.model.clone(),
            agent.resume_session_id.as_deref(),
        );
        validate_agent_model(
            &self.agent_registry,
            &agent.provider,
            launch_model.as_deref(),
        )?;

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
                if let Some(ref model) = launch_model {
                    pane.agent_session_model = Some(model.clone());
                }
                if let Some(ref rtid) = agent.report_target_pane_id {
                    pane.report_target_pane_id = Some(rtid.clone());
                }
                if let Some(ref sb) = agent.spawned_by_pane_id {
                    if !sb.is_empty() {
                        pane.spawned_by_pane_id = Some(sb.clone());
                    }
                }
            }
            result
        };

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

        // Wide PTY: ConPTY wrapping at col 120 corrupts claude stream-json mid-line.
        let pty_id = {
            let mut pty = self.pty.lock();
            pty.spawn(None, resolved_cwd, 9999, 40, env_vars)
                .map(|(id, _)| id)?
        };

        if let Some(orphan) = self.mux.lock().link_pty(&pane_id, pty_id.clone()) {
            self.kill_ptys(&[orphan]);
        }
        self.agent_run_pty_map
            .lock()
            .insert(pane_id.clone(), pty_id.clone());
        // Register run for auto-complete via stream-json result event / idle / timeout. Cron kind is resolved later (after the cron registry links
        // pane→run).
        if let Some(ref worker_id) = agent.worker_id {
            // Dreamer (REM phase) needs a longer timeout than the default worker — hub decomposition + diary + classification all in one session takes a
            // while. Pick by intent: any worker whose intent starts with "dreaming:" gets the dreaming-specific timeout.
            let mem_cfg = crate::config::memory_config();
            let intent = self
                .workers
                .get(worker_id)
                .map(|w| w.intent.clone())
                .unwrap_or_default();
            let dreaming = crate::mem::workers::is_dreaming_intent(&intent);
            let timeout = if dreaming {
                mem_cfg.dreaming_timeout_seconds
            } else {
                mem_cfg.worker_timeout_seconds
            };
            // Surface the timeout choice so reports of "dreaming worker died at 150s" (= worker_timeout_seconds, not dreaming_timeout_seconds=1800) can
            // diagnosed: either intent lookup returned empty, or is_dreaming_intent didn't match.
            log::info!(
                "[run-watcher] register worker={} intent='{}' dreaming={} timeout={}s",
                worker_id,
                intent,
                dreaming,
                timeout
            );
            self.run_watcher.register(
                self.clone(),
                pty_id.clone(),
                pane_id.clone(),
                crate::agent::run_watcher::AgentRunKind::MemWorker {
                    worker_id: worker_id.clone(),
                },
                timeout,
            );
        }
        self.emit_state_changed();

        let model_args: Vec<String> = launch_model
            .as_ref()
            .and_then(|m| {
                self.agent_registry
                    .with_provider(&agent.provider, |p| p.model_launch_args(m))
            })
            .unwrap_or_default();

        let wrapped_task = agent.task.as_ref().map(|t| {
            if let Some(ref preamble) = agent.task_preamble {
                format!("{}{}", preamble, t)
            } else {
                t.clone()
            }
        });

        let svc_outer = std::sync::Arc::new(self.clone());
        let pane_id_outer = pane_id.clone();
        let pty_id_outer = pty_id.clone();
        let provider = agent.provider.clone();
        let task_for_thread = wrapped_task.clone();
        std::thread::spawn(move || {
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
                        true,
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
                headless: true,
                session_id: None,
            };
            let launch_cmd = svc_outer
                .agent_registry
                .with_provider(&provider, |p| p.build_launch_command(&launch_params))
                .unwrap_or_else(|| format!("{} --dangerously-skip-permissions", provider));

            match svc_outer.resolve_pty_id(&pane_id_outer) {
                Ok(pty_id) => {
                    if let Err(e) = svc_outer.send_shell_spawn(&pty_id, &launch_cmd) {
                        log::error!("[spawn.hidden] shell spawn failed: {}", e);
                    }
                }
                Err(e) => log::error!("[spawn.hidden] no pty for pane {}: {}", pane_id_outer, e),
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

#[cfg(test)]
#[path = "spawn_tests.rs"]
mod tests;
