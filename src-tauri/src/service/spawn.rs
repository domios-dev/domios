use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tauri::Manager;

use super::orchestrator::{build_agent_command, launch_agent_in_background};
use super::ServiceContext;
use crate::agent::registry::AgentRegistry;

pub(crate) use super::spawn_cli::{cli_binary_on_path, provider_cli_install_warning};
pub(crate) use super::spawn_params::chatbackend_spawn_config;

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

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaneCommand {
    #[default]
    Shell,
    Agent(AgentSpawnSpec),
    /// A pane driven by a plugin's `chatBackend` capability — no PTY, no shell.
    ChatBackend {
        plugin_id: String,
        #[serde(default)]
        config: serde_json::Value,
    },
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

fn merge_warnings(a: Option<String>, b: Option<String>) -> Option<String> {
    match (a, b) {
        (Some(a), Some(b)) => Some(format!("{}\n{}", a, b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
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
            (PaneVisibility::Visible, PaneCommand::ChatBackend { .. }) => {
                self.spawn_chatbackend_impl(spec)
            }
            (PaneVisibility::Hidden, PaneCommand::ChatBackend { .. }) => {
                self.spawn_chatbackend_impl(spec)
            }
        }
    }
}

pub(crate) fn validate_agent_model(
    registry: &AgentRegistry,
    provider_id: &str,
    model: Option<&str>,
) -> Result<(), String> {
    let _ = registry;
    if model.map(str::trim).is_some_and(|m| !m.is_empty()) {
        let _ = crate::agent::model_catalog::resolve_model(provider_id, model);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelFallbackNotice {
    pub requested: String,
    pub using: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FreshAgentModelResolution {
    pub model: Option<String>,
    pub fallback_notice: Option<ModelFallbackNotice>,
}

pub(crate) fn model_fallback_notice_title(notice: &ModelFallbackNotice) -> String {
    format!(
        "model {} unavailable -> using {}",
        notice.requested, notice.using
    )
}

pub(crate) fn resolve_fresh_agent_model(
    registry: &AgentRegistry,
    provider_id: &str,
    requested: Option<String>,
    resume_session_id: Option<&str>,
) -> Option<String> {
    resolve_fresh_agent_model_with_notice(registry, provider_id, requested, resume_session_id).model
}

pub(crate) fn resolve_fresh_agent_model_with_notice(
    registry: &AgentRegistry,
    provider_id: &str,
    requested: Option<String>,
    resume_session_id: Option<&str>,
) -> FreshAgentModelResolution {
    if resume_session_id.is_some() {
        return FreshAgentModelResolution {
            model: requested.and_then(non_empty_model),
            fallback_notice: None,
        };
    }
    if let Some(model) = requested.and_then(non_empty_model) {
        let resolved = crate::agent::model_catalog::resolve_model(provider_id, Some(&model));
        let fallback_notice = if resolved.fell_back {
            log::warn!(
                "[spawn] requested model '{}' unavailable for provider '{}'; using '{}'",
                model,
                provider_id,
                resolved.id
            );
            Some(ModelFallbackNotice {
                requested: model,
                using: resolved.id.clone(),
            })
        } else {
            None
        };
        return FreshAgentModelResolution {
            model: non_empty_model(resolved.id),
            fallback_notice,
        };
    }
    if let Some(model) = crate::config::provider_config(provider_id)
        .default_model
        .and_then(non_empty_model)
    {
        let resolved = crate::agent::model_catalog::resolve_model(provider_id, Some(&model));
        if !resolved.fell_back {
            return FreshAgentModelResolution {
                model: non_empty_model(resolved.id),
                fallback_notice: None,
            };
        }
        log::warn!(
            "[spawn] ignoring stale default model '{}' for provider '{}'",
            model,
            provider_id
        );
    }
    let provider_default = registry
        .with_provider(provider_id, |p| p.default_model())
        .flatten()
        .and_then(non_empty_model);
    let resolved =
        crate::agent::model_catalog::resolve_model(provider_id, provider_default.as_deref());
    FreshAgentModelResolution {
        model: non_empty_model(resolved.id),
        fallback_notice: None,
    }
}

fn non_empty_model(model: String) -> Option<String> {
    let trimmed = model.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
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

        self.require_agent_provider_ready(&agent_cmd)?;
        let cli_warning = provider_cli_install_warning(&self.agent_registry, &agent_cmd)?;
        if let Some(ref warning) = cli_warning {
            log::warn!("[spawn] {}", warning);
        }
        let model_resolution = resolve_fresh_agent_model_with_notice(
            &self.agent_registry,
            &agent_cmd,
            agent.model.clone(),
            agent.resume_session_id.as_deref(),
        );
        let launch_model = model_resolution.model;
        validate_agent_model(&self.agent_registry, &agent_cmd, launch_model.as_deref())?;
        if let Some(ref notice) = model_resolution.fallback_notice {
            self.create_notification(model_fallback_notice_title(notice), None, Some("warning"));
        }

        let title = spec
            .title
            .clone()
            .or_else(|| agent.agent_name.clone())
            .unwrap_or_else(|| agent_cmd.clone());

        let (ws_id, ws_cwd) = {
            let mux = self.mux.write();
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

        let (resolved_cwd, cwd_warning) = Self::resolve_agent_cwd(&spec.cwd, &ws_cwd);
        let warning = merge_warnings(cwd_warning, cli_warning);

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
            let mut mux = self.mux.write();
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
            if let Some(pane) = self.mux.write().get_pane_mut(&pane_id) {
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
        // Tag with the spawner's origin prefix uniformly for every provider; delivery handles the marker per provider (TUI-typed for PtyInput, positional after `--` for inline-flag).
        let role_suffixed_task =
            role_suffixed_task.map(|t| match self.spawn_task_prefix(&pane_id) {
                Some(pfx) => format!("{}{}", pfx, t),
                None => t,
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
            let mux = self.mux.write();
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
            let mut mux = self.mux.write();
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
                .write()
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
            let mux = self.mux.write();
            spec.workspace_id
                .clone()
                .or_else(|| mux.snapshot().active_workspace_id.clone())
                .or_else(|| mux.list_workspaces().first().map(|w| w.id.clone()))
                .ok_or("No workspace for hidden shell")?
        };
        let ws_cwd = {
            let mux = self.mux.write();
            mux.get_workspace(&ws_id).and_then(|ws| ws.cwd.clone())
        };
        let resolved_cwd = spec.cwd.clone().or(ws_cwd);

        let (tab_id, pane_id) = {
            let mut mux = self.mux.write();
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
            "CODETERM_SESSION_BINDING_NONCE".into(),
            self.ensure_session_binding_nonce(&pane_id),
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

        let pty_id = {
            let mut pty = self.pty.lock();
            pty.spawn(None, resolved_cwd, 120, 40, env_vars)
                .map(|(id, _)| id)?
        };
        if let Some(orphan) = self.mux.write().link_pty(&pane_id, pty_id.clone()) {
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
        self.require_agent_provider_ready(&agent.provider)?;
        let cli_warning = provider_cli_install_warning(&self.agent_registry, &agent.provider)?;
        if let Some(ref warning) = cli_warning {
            log::warn!("[spawn.hidden] {}", warning);
        }
        let model_resolution = resolve_fresh_agent_model_with_notice(
            &self.agent_registry,
            &agent.provider,
            agent.model.clone(),
            agent.resume_session_id.as_deref(),
        );
        let launch_model = model_resolution.model;
        validate_agent_model(
            &self.agent_registry,
            &agent.provider,
            launch_model.as_deref(),
        )?;
        if let Some(ref notice) = model_resolution.fallback_notice {
            self.create_notification(model_fallback_notice_title(notice), None, Some("warning"));
        }

        let ws_id = {
            let mux = self.mux.write();
            spec.workspace_id
                .clone()
                .or_else(|| mux.snapshot().active_workspace_id.clone())
                .or_else(|| mux.list_workspaces().first().map(|w| w.id.clone()))
                .ok_or("No workspace for hidden spawn")?
        };
        let ws_cwd = {
            let mux = self.mux.write();
            mux.get_workspace(&ws_id).and_then(|ws| ws.cwd.clone())
        };
        let resolved_cwd = spec.cwd.clone().or(ws_cwd);

        let (tab_id, pane_id) = {
            let mut mux = self.mux.write();
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
        env_vars.insert(
            "CODETERM_SESSION_BINDING_NONCE".into(),
            self.ensure_session_binding_nonce(&pane_id),
        );
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

        // Hidden agents now launch as ordinary interactive claude (no -p/stream-json), so use the same 120x40 PTY a visible agent gets.
        let pty_id = {
            let mut pty = self.pty.lock();
            pty.spawn(None, resolved_cwd, 120, 40, env_vars)
                .map(|(id, _)| id)?
        };

        if let Some(orphan) = self.mux.write().link_pty(&pane_id, pty_id.clone()) {
            self.kill_ptys(&[orphan]);
        }
        self.agent_run_pty_map
            .lock()
            .insert(pane_id.clone(), pty_id.clone());
        // Register run for auto-complete via the standard agent rails: `mem worker report` / Stop hook / idle / timeout backstop. Cron kind is resolved
        // later (after the cron registry links pane→run).
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

            // headless:false → ordinary interactive claude, task delivered as a positional after `--` (same launch a visible agent gets). Completion comes
            // from the standard agent rails, not a stream-json result event.
            let launch_params = crate::agent::provider::AgentLaunchParams {
                agent_name: None,
                task: task_for_thread,
                args: if model_args.is_empty() {
                    None
                } else {
                    Some(model_args)
                },
                skip_permissions: true,
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
            warning: cli_warning,
        })
    }
}

#[cfg(test)]
#[path = "spawn_tests.rs"]
mod tests;
