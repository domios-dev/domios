use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::orchestrator::launch_agent_in_background_with_env;
use super::ServiceContext;
use crate::agent::registry::AgentRegistry;

pub(crate) use super::spawn_cli::{
    cli_binary_on_path, create_cleared_agent_tab, provider_launch_preflight,
};
#[cfg(test)]
pub(crate) use super::spawn_params::validate_watcher_spawn_target;
pub(crate) use super::spawn_params::{
    chatbackend_spawn_config, reject_direct_watcher_chatbackend_config,
};

#[path = "spawn_model.rs"]
mod spawn_model;
pub use spawn_model::ModelFallbackNotice;
pub(crate) use spawn_model::{
    attach_planned_model, model_fallback_notice_title, normalize_session_model,
    persist_model_launch_identity, resolve_fresh_agent_model,
    resolve_fresh_agent_model_with_notice, unrequested_substitution_severity,
    CatalogBackgroundModelPort,
};

#[path = "spawn_hidden_delivery.rs"]
mod spawn_hidden_delivery;
use crate::multiplexer::types::TabCloseReason;
use spawn_hidden_delivery::deliver_hidden_pty_task;

#[path = "spawn_identity.rs"]
mod spawn_identity;
#[cfg(test)]
pub(crate) use spawn_identity::task_spawn_handle;
pub(crate) use spawn_identity::{
    now_unix_secs, params_caller_tab_id, roleless_spawner, spawn_caller_tab_id,
    task_binding_from_spawn,
};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SpawnTabSpec {
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub parent_tab_id: Option<String>,
    #[serde(default)]
    pub team_id: Option<String>,
    #[serde(default)]
    pub creator_tab_id: Option<String>,
    #[serde(default)]
    pub detach_from_team: bool,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub visibility: TabVisibility,
    /// Audience axis for input-request notifications; declared here, never
    /// inferred from provider id, title, or prompt text.
    #[serde(default)]
    pub audience: crate::multiplexer::types::TabAudience,
    #[serde(default)]
    pub eager: bool,
    pub command: TabCommand,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TabVisibility {
    #[default]
    Visible,
    Hidden,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TabCommand {
    #[default]
    Shell,
    Agent(AgentSpawnSpec),
    /// A tab driven by a plugin's `chatBackend` capability — no PTY, no shell.
    ChatBackend {
        plugin_id: String,
        #[serde(default)]
        config: serde_json::Value,
        #[serde(default)]
        role: Option<String>,
        #[serde(default)]
        role_profile: Option<String>,
        #[serde(default)]
        can_spawn: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        watcher: Option<crate::multiplexer::types::WatcherSpec>,
    },
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AgentSpawnSpec {
    pub provider: String,
    #[serde(default)]
    pub planned_model: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default, skip_deserializing)]
    pub(crate) planned_reasoning_effort: Option<String>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    #[serde(default)]
    pub task: Option<String>,
    #[serde(default)]
    pub agent_name: Option<String>,
    #[serde(default)]
    pub extra_args: Vec<String>,
    #[serde(default, skip_deserializing)]
    pub provisioned_cwd: Option<String>,
    #[serde(default, skip_deserializing)]
    pub(crate) lane_setup_command: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub role_profile: Option<String>,
    #[serde(default)]
    pub can_spawn: Option<bool>,
    #[serde(default)]
    pub report_target_tab_id: Option<String>,
    #[serde(default)]
    pub worker_id: Option<String>,
    #[serde(default)]
    pub spawned_by: Option<String>,
    #[serde(default)]
    pub extra_env: HashMap<String, String>,
    #[serde(default)]
    pub task_preamble: Option<String>,
    #[serde(default)]
    pub spawned_by_tab_id: Option<String>,
    #[serde(default)]
    pub resume_session_id: Option<String>,
    #[serde(default)]
    pub activate: Option<bool>,
}

fn merge_warnings(a: Option<String>, b: Option<String>) -> Option<String> {
    match (a, b) {
        (Some(a), Some(b)) => Some(format!("{}\n{}", a, b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

/// Explicit `workspace_id` wins, then the spawn lineage: declared parent, then
/// the caller's own tab. The focused workspace is a last resort for callers with
/// no tab identity (UI clicks, external API) — an agent must never land its
/// children in whatever workspace the user happens to be looking at.
pub(crate) fn resolve_spawn_workspace_id(
    requested_workspace_id: Option<String>,
    parent_tab_id: Option<&str>,
    caller_tab_id: Option<&str>,
    workspace_of_tab: impl Fn(&str) -> Option<String>,
    active_workspace_id: Option<String>,
    fallback_workspace_id: Option<String>,
) -> Result<String, String> {
    requested_workspace_id
        .or_else(|| parent_tab_id.and_then(&workspace_of_tab))
        .or_else(|| caller_tab_id.and_then(&workspace_of_tab))
        .or(active_workspace_id)
        .or(fallback_workspace_id)
        .ok_or_else(|| "No workspace found".to_string())
}

pub(crate) fn should_default_agent_tab_to_chat(
    visibility: TabVisibility,
    provider_supports_chat_ui: bool,
) -> bool {
    visibility == TabVisibility::Visible && provider_supports_chat_ui
}

#[derive(Debug, Clone, Serialize)]
pub struct TabHandle {
    pub tab_id: String,
    pub pty_id: Option<String>,
    pub workspace_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_fallback: Option<ModelFallbackNotice>,
    /// Advisory workspace tab-budget nudge (task_4d649dfd). Set once the
    /// post-create workspace tab count crosses the soft budget. Never blocks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_notice: Option<super::tab_budget::TabBudgetNotice>,
}

impl ServiceContext {
    pub fn spawn_tab(&self, spec: SpawnTabSpec) -> Result<TabHandle, String> {
        self.spawn_tab_with_ready(spec, None)
    }

    pub fn spawn_tab_with_ready(
        &self,
        spec: SpawnTabSpec,
        ready_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    ) -> Result<TabHandle, String> {
        self.spawn_tab_with_ready_and_state_change(spec, ready_tx, true)
    }

    pub(crate) fn spawn_tab_with_ready_and_state_change(
        &self,
        spec: SpawnTabSpec,
        ready_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
        emit_state_change: bool,
    ) -> Result<TabHandle, String> {
        match (&spec.visibility, &spec.command) {
            (TabVisibility::Visible, TabCommand::Agent(_)) => {
                self.spawn_visible_agent_impl(spec, ready_tx)
            }
            (TabVisibility::Hidden, TabCommand::Agent(_)) => self.spawn_hidden_agent_impl(spec),
            (TabVisibility::Visible, TabCommand::Shell) => self.spawn_visible_shell_impl(spec),
            (TabVisibility::Hidden, TabCommand::Shell) => self.spawn_hidden_shell_impl(spec),
            (TabVisibility::Visible, TabCommand::ChatBackend { .. }) => {
                super::permission_policy::configured_full_access()?;
                self.spawn_chatbackend_impl_with_state_change(spec, emit_state_change)
            }
            (TabVisibility::Hidden, TabCommand::ChatBackend { .. }) => {
                super::permission_policy::configured_full_access()?;
                self.spawn_chatbackend_impl_with_state_change(spec, emit_state_change)
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

/// Structurally validate a requested reasoning effort against the provider's
/// declared capability. Empty/absent is always valid (no selection). An
/// unsupported value returns the provider's structured rejection message and the
/// caller aborts the spawn — never a silent downgrade. An unknown provider is an
/// error too: we cannot honestly claim support we can't verify.
pub(crate) fn validate_agent_reasoning_effort(
    registry: &AgentRegistry,
    provider_id: &str,
    effort: Option<&str>,
) -> Result<(), String> {
    let Some(effort) = effort.map(str::trim).filter(|e| !e.is_empty()) else {
        return Ok(());
    };
    match registry.with_provider(provider_id, |p| p.validate_reasoning_effort(Some(effort))) {
        Some(Ok(())) => Ok(()),
        Some(Err(rejection)) => Err(rejection.to_string()),
        None => Err(format!(
            "unknown provider '{provider_id}': cannot validate reasoning effort '{effort}'"
        )),
    }
}

impl ServiceContext {
    pub(crate) fn spawn_visible_agent_impl(
        &self,
        spec: SpawnTabSpec,
        ready_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    ) -> Result<TabHandle, String> {
        let agent = match &spec.command {
            TabCommand::Agent(a) => a.clone(),
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
        let permission_bypass_authorization =
            super::permission_policy::configured_permission_bypass(
                &self.agent_registry,
                &agent_cmd,
            )?;
        let permission_bypass = permission_bypass_authorization.is_some();

        self.require_agent_provider_ready(&agent_cmd)?;
        let clearance = provider_launch_preflight(&self.agent_registry, &agent_cmd)
            .map_err(|e| e.to_string())?;
        let cli_warning = clearance.warning.clone();
        if let Some(ref warning) = cli_warning {
            log::warn!("[spawn] {}", warning);
        }
        let mut model_resolution = resolve_fresh_agent_model_with_notice(
            &self.agent_registry,
            &agent_cmd,
            agent.model.clone(),
            agent.resume_session_id.as_deref(),
        );
        attach_planned_model(&mut model_resolution, agent.planned_model.as_deref());
        let launch_model = model_resolution.model.clone();
        validate_agent_model(&self.agent_registry, &agent_cmd, launch_model.as_deref())?;
        validate_agent_reasoning_effort(
            &self.agent_registry,
            &agent_cmd,
            agent.reasoning_effort.as_deref(),
        )?;
        if let Some(ref notice) = model_resolution.fallback_notice {
            self.create_notification(model_fallback_notice_title(notice), None, Some("warning"));
        }

        let (ws_id, ws_cwd, ws_provisioned_cwd) = {
            let mux = self.mux.write();
            let id = resolve_spawn_workspace_id(
                spec.workspace_id.clone(),
                spec.parent_tab_id.as_deref(),
                spawn_caller_tab_id(&agent, &spec),
                |pid| mux.find_workspace_for_tab(pid),
                mux.get_active_workspace().map(|w| w.id.clone()),
                mux.list_workspaces().first().map(|w| w.id.clone()),
            )?;
            let workspace = mux.get_workspace(&id);
            let cwd = workspace.and_then(|ws| ws.cwd.clone());
            let provisioned_cwd = workspace.and_then(|ws| ws.provisioned_cwd.clone());
            (id, cwd, provisioned_cwd)
        };

        // Authorized before the tab exists: a non-member caller must never leave
        // an orphan agent behind.
        let team_attachment = super::team::resolve_spawn_attachment(
            &self.mux.read(),
            spec.team_id.as_deref(),
            spec.creator_tab_id.as_deref(),
            &ws_id,
        )?;

        let (resolved_cwd, cwd_warning) = Self::resolve_agent_cwd(&spec.cwd, &ws_cwd);
        let warning = merge_warnings(cwd_warning, cli_warning);

        let default_chat_view = should_default_agent_tab_to_chat(
            spec.visibility,
            self.agent_registry
                .with_provider(&agent_cmd, |p| p.supports_chat_ui())
                .unwrap_or(false),
        );

        let tab_id = {
            let mut mux = self.mux.write();
            let result = create_cleared_agent_tab(&mut mux, &ws_id, &clearance)?;
            if let Some(ref t) = spec.title {
                if !t.trim().is_empty() {
                    let _ = mux.rename_tab_at_level(
                        &result,
                        t,
                        crate::multiplexer::types::TitleLevel::User,
                    );
                }
            }
            if let Some(ref dir) = resolved_cwd {
                if let Some(tab) = mux.get_tab_mut(&result) {
                    tab.cwd = Some(dir.clone());
                }
            }
            let provisioned_cwd = agent.provisioned_cwd.clone().or_else(|| {
                super::directory_trust::inherited_workspace_provenance(
                    resolved_cwd.as_deref(),
                    ws_provisioned_cwd.as_deref(),
                )
            });
            if let Some(provisioned_cwd) = provisioned_cwd {
                if let Some(tab) = mux.get_tab_mut(&result) {
                    tab.provisioned_cwd = Some(provisioned_cwd);
                }
            }
            if let Some(tab) = mux.get_tab_mut(&result) {
                tab.role_profile = agent.role_profile.clone();
                tab.audience = spec.audience;
            }
            if let Some(ref description) = spec.description {
                let _ = mux.set_tab_description(&result, Some(description.clone()));
            }
            if let Some(ref rtid) = agent.report_target_tab_id {
                if let Some(tab) = mux.get_tab_mut(&result) {
                    tab.report_target_tab_id = Some(rtid.clone());
                }
            }
            if let Some(ref sb) = agent.spawned_by_tab_id {
                if !sb.is_empty() {
                    if let Some(tab) = mux.get_tab_mut(&result) {
                        tab.spawned_by_tab_id = Some(sb.clone());
                    }
                }
            }
            if let Some(ref wid) = agent.worker_id {
                if let Some(tab) = mux.get_tab_mut(&result) {
                    tab.worker_id = Some(wid.clone());
                }
            }
            if let Some(tab) = mux.get_tab_mut(&result) {
                tab.auto_approve = permission_bypass;
            }
            if default_chat_view {
                if let Some(tab) = mux.get_tab_mut(&result) {
                    tab.chat_view = true;
                }
            }
            // real agent name was supplied — the raw provider id is never a title.
            if let Some((title, level)) = super::spawn_title::creation_title_override(
                spec.title.as_deref(),
                agent.agent_name.as_deref(),
            ) {
                if let Err(e) = mux.rename_tab_at_level(&result, &title, level) {
                    log::warn!("[spawn] rename_tab failed: {}", e);
                }
            }
            if agent.activate.unwrap_or(false) {
                let _ = mux.set_active_tab(&ws_id, &result);
            }
            result
        };

        // Identity before authority: a role without a launchable provider
        // restores as a role-bearing shell.
        self.set_and_lock_tab_agent_type_from(
            &tab_id,
            agent_cmd.clone(),
            crate::session::lifecycle::AgentTypeWriteOrigin::Spawn,
            agent.spawned_by_tab_id.as_deref(),
        );
        let startup_supervised = self.prepare_agent_startup_supervision(&tab_id);
        // `can_spawn` is only forced when the caller explicitly passed
        // --can-spawn (Some), preserving the kind's default.
        if let Some(ref role_str) = agent.role {
            if let Err(e) = self.set_tab_role_with_profile(
                &tab_id,
                role_str,
                agent.role_profile.as_deref(),
                spec.parent_tab_id.as_deref(),
                agent.can_spawn,
            ) {
                self.rollback_provisional_tab_via_close(&tab_id);
                self.emit_state_changed();
                return Err(e);
            }
            if role_str == "Worker" || role_str == "Manager" {
                if let Some(ref parent) = spec.parent_tab_id {
                    if let Err(e) = self.expect_spawn_parent_link(&tab_id, parent) {
                        self.rollback_provisional_tab_via_close(&tab_id);
                        self.emit_state_changed();
                        return Err(e);
                    }
                }
            }
        } else {
            // Roleless spawn: bind the spawner as parent and audit it, so the
            // pane is never untracked and unattributable.
            let spawner = roleless_spawner(&agent, &spec);
            if let Some(ref parent) = spawner {
                let mut mux = self.mux.write();
                if let Some(tab) = mux.get_tab_mut(&tab_id) {
                    if tab.parent_tab_id.is_none() {
                        tab.parent_tab_id = Some(parent.clone());
                    }
                }
                let _ = mux.mark_roleless_tracked(&tab_id);
            } else {
                let _ = self.mux.write().mark_roleless_tracked(&tab_id);
            }
            if let Some(parent) = spawner {
                let title = self
                    .mux
                    .read()
                    .get_tab(&tab_id)
                    .map(|t| t.title.clone())
                    .unwrap_or_default();
                self.report_store.lock().record_spawn_audit(
                    &parent,
                    &tab_id,
                    &title,
                    now_unix_secs(),
                );
            }
        }
        // After role placement, which owns parent/team for a roled tab, and
        // before launch so the PTY inherits CODETERM_TEAM_ID.
        if let Some(attachment) = &team_attachment {
            let applied = {
                let mut mux = self.mux.write();
                super::team::apply_team_attachment(&mut mux, attachment, &tab_id)
            };
            if let Err(error) = applied {
                let _ = self
                    .mux
                    .write()
                    .close_tab_with_reason(&tab_id, TabCloseReason::SpawnRollback);
                self.emit_state_changed();
                return Err(error);
            }
        }
        if let Some(binding) = task_binding_from_spawn(&agent) {
            let _ = self.mux.write().set_task_binding_once(&tab_id, binding);
        }
        self.ensure_agent_pad(&tab_id).inspect_err(|_| {
            self.rollback_provisional_tab(&ws_id, &tab_id);
        })?;
        let launch_session_id = if agent.resume_session_id.is_none() {
            self.agent_registry
                .with_provider(&agent_cmd, |p| p.session_id_injection())
                .filter(|mode| !matches!(mode, crate::agent::provider::SessionIdInjection::None))
                .map(|_| uuid::Uuid::new_v4().to_string())
        } else {
            None
        };
        if let Some(ref sid) = agent.resume_session_id {
            self.set_agent_session_id(&tab_id, Some(sid.clone()));
        } else if let Some(ref sid) = launch_session_id {
            self.set_agent_session_id(&tab_id, Some(sid.clone()));
        }
        if let Some(tab) = self.mux.write().get_tab_mut(&tab_id) {
            persist_model_launch_identity(tab, agent.planned_model.as_deref(), &model_resolution);
            super::reasoning_effort::persist_launch_identity(
                tab,
                agent.planned_reasoning_effort.as_deref(),
                agent.reasoning_effort.as_deref(),
            );
        }
        self.emit_state_changed();

        let preamble_wrapped_task = agent.task.as_ref().map(|t| {
            if let Some(ref preamble) = agent.task_preamble {
                format!("{}{}", preamble, t)
            } else {
                t.clone()
            }
        });
        let (system_prompt_capable, native_starter_capable) = self
            .agent_registry
            .with_provider(
                &agent_cmd,
                crate::service::launch_delivery::visible_spawn_capabilities,
            )
            .unwrap_or((false, false));
        let task_selection = crate::service::launch_delivery::select_visible_spawn_task(
            agent.role.as_deref(),
            preamble_wrapped_task.as_deref(),
            system_prompt_capable,
            native_starter_capable,
        );
        let role_suffixed_task = super::orchestrator::role_launch_task(
            task_selection.provider_launch,
            agent.role.as_deref(),
            agent.role_profile.as_deref(),
            system_prompt_capable,
        );
        let task_prefix = self.spawn_task_prefix(&tab_id);
        let role_suffixed_task = role_suffixed_task.map(|task| match task_prefix.as_deref() {
            Some(prefix) => format!("{prefix}{task}"),
            None => task,
        });
        let confirmed_first_turn = if startup_supervised {
            crate::service::launch_delivery::startup_handshake_confirmed_first_turn(
                native_starter_capable,
            )
            .map(|prompt| {
                let text = self
                    .agent_registry
                    .with_provider(&agent_cmd, |provider| provider.starter_prompt_text(prompt))
                    .flatten();
                crate::service::launch_delivery::mark_starter_prompt(
                    prompt,
                    text.unwrap_or_default(),
                )
            })
        } else {
            match task_selection.confirmed_first_turn {
                Some(
                    crate::service::launch_delivery::VisibleSpawnFirstTurn::AssignedTaskStarter,
                ) => {
                    let prompt = crate::agent::provider::StarterPrompt::AssignedTask;
                    let text = self
                        .agent_registry
                        .with_provider(&agent_cmd, |provider| provider.starter_prompt_text(prompt))
                        .flatten();
                    Some(crate::service::launch_delivery::mark_starter_prompt(
                        prompt,
                        crate::service::launch_delivery::assigned_task_starter(text.as_deref()),
                    ))
                }
                Some(crate::service::launch_delivery::VisibleSpawnFirstTurn::Task(task)) => Some(
                    task_prefix
                        .as_deref()
                        .map_or_else(|| task.to_string(), |prefix| format!("{prefix}{task}")),
                ),
                None => None,
            }
        };

        let mut legacy_params = spec.to_agent_params()?;
        if startup_supervised {
            legacy_params.starter_prompt = crate::agent::provider::StarterPrompt::StartupHandshake;
        } else if let Some(starter) = task_selection.provider_starter {
            legacy_params.starter_prompt = starter;
        }
        legacy_params.skip_permissions = Some(permission_bypass);
        legacy_params.permission_bypass_authorization = permission_bypass_authorization;
        legacy_params.model = launch_model;
        legacy_params.structured_prompt_server =
            crate::service::structured_prompt::ensure_for_spawn(
                self,
                &agent_cmd,
                &tab_id,
                &resolved_cwd.clone().unwrap_or_default(),
            );
        let provider_fallback_delivery = self
            .agent_registry
            .with_provider(&agent_cmd, |provider| {
                matches!(
                    provider.task_delivery(),
                    crate::agent::provider::TaskDelivery::PtyInput
                ) && !system_prompt_capable
            })
            .unwrap_or(true);
        let fallback_task = confirmed_first_turn.as_deref().or_else(|| {
            provider_fallback_delivery
                .then_some(role_suffixed_task.as_deref())
                .flatten()
        });
        let fallback_spec = crate::service::launch_delivery::fallback_delivery_spec(
            None,
            fallback_task,
            self.agent_registry
                .with_provider(&agent_cmd, |provider| {
                    provider.starter_prompt_text(legacy_params.starter_prompt)
                })
                .flatten()
                .as_deref(),
        );
        if let Some(spec) = fallback_spec.as_ref() {
            if let Err(error) =
                self.create_pending_delivery(&tab_id, spec.channel.clone(), &spec.payload)
            {
                let _ = self.close_tab_with_reason(&tab_id, TabCloseReason::SpawnRollback);
                return Err(error.to_string());
            }
        }
        let launch_cmd = visible_agent_launch_command(
            &tab_id,
            &agent_cmd,
            &legacy_params,
            role_suffixed_task.as_deref(),
            launch_session_id.as_deref(),
            agent.lane_setup_command.as_deref(),
            &self.agent_registry,
        );
        launch_agent_in_background_with_env(
            self.clone(),
            tab_id.clone(),
            launch_cmd,
            agent_cmd.clone(),
            crate::service::launch_diagnostics::AgentLaunchOrigin::VisibleSpawn,
            role_suffixed_task,
            fallback_spec,
            agent.extra_env.clone(),
            ready_tx,
        );

        let budget_notice = self.workspace_tab_budget_notice(&ws_id);
        Ok(TabHandle {
            tab_id,
            pty_id: None,
            workspace_id: ws_id,
            description: spec.description,
            worker_id: agent.worker_id.clone(),
            warning,
            model_fallback: model_resolution.fallback_notice,
            budget_notice,
        })
    }

    pub(crate) fn spawn_hidden_agent_impl(&self, spec: SpawnTabSpec) -> Result<TabHandle, String> {
        use std::collections::HashMap;

        let agent = match &spec.command {
            TabCommand::Agent(a) => a.clone(),
            _ => return Err("spawn_hidden_agent_impl called without Agent command".into()),
        };

        if agent.provider.is_empty() {
            return Err("AgentSpawnSpec.provider is required for hidden spawns".into());
        }
        self.require_agent_provider_ready(&agent.provider)?;
        let permission_bypass_authorization =
            super::permission_policy::configured_permission_bypass(
                &self.agent_registry,
                &agent.provider,
            )?;
        let permission_bypass = permission_bypass_authorization.is_some();
        let clearance = provider_launch_preflight(&self.agent_registry, &agent.provider)
            .map_err(|e| e.to_string())?;
        let cli_warning = clearance.warning.clone();
        if let Some(ref warning) = cli_warning {
            log::warn!("[spawn.hidden] {}", warning);
        }
        let mut model_resolution = resolve_fresh_agent_model_with_notice(
            &self.agent_registry,
            &agent.provider,
            agent.model.clone(),
            agent.resume_session_id.as_deref(),
        );
        attach_planned_model(&mut model_resolution, agent.planned_model.as_deref());
        let launch_model = model_resolution.model.clone();
        validate_agent_model(
            &self.agent_registry,
            &agent.provider,
            launch_model.as_deref(),
        )?;
        validate_agent_reasoning_effort(
            &self.agent_registry,
            &agent.provider,
            agent.reasoning_effort.as_deref(),
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
        let (ws_cwd, ws_provisioned_cwd) = {
            let mux = self.mux.write();
            let workspace = mux.get_workspace(&ws_id);
            (
                workspace.and_then(|ws| ws.cwd.clone()),
                workspace.and_then(|ws| ws.provisioned_cwd.clone()),
            )
        };
        let resolved_cwd = spec.cwd.clone().or(ws_cwd);

        let tab_id = {
            let mut mux = self.mux.write();
            let result = create_cleared_agent_tab(&mut mux, &ws_id, &clearance)?;
            if let Some(tab) = mux.get_tab_mut(&result) {
                tab.hidden = true;
                tab.show_in_tabbar = false;
                tab.audience = spec.audience;
                tab.role_profile = agent.role_profile.clone();
                if let Some(ref wid) = agent.worker_id {
                    tab.worker_id = Some(wid.clone());
                }
                tab.auto_approve = permission_bypass;
                if let Some(ref dir) = resolved_cwd {
                    tab.cwd = Some(dir.clone());
                }
                tab.provisioned_cwd = agent.provisioned_cwd.clone().or_else(|| {
                    super::directory_trust::inherited_workspace_provenance(
                        resolved_cwd.as_deref(),
                        ws_provisioned_cwd.as_deref(),
                    )
                });
                persist_model_launch_identity(
                    tab,
                    agent.planned_model.as_deref(),
                    &model_resolution,
                );
                super::reasoning_effort::persist_launch_identity(
                    tab,
                    agent.planned_reasoning_effort.as_deref(),
                    agent.reasoning_effort.as_deref(),
                );
                if let Some(ref rtid) = agent.report_target_tab_id {
                    tab.report_target_tab_id = Some(rtid.clone());
                }
                if let Some(ref sb) = agent.spawned_by_tab_id {
                    if !sb.is_empty() {
                        tab.spawned_by_tab_id = Some(sb.clone());
                    }
                }
            }
            result
        };

        self.set_and_lock_tab_agent_type_from(
            &tab_id,
            agent.provider.clone(),
            crate::session::lifecycle::AgentTypeWriteOrigin::Spawn,
            agent.spawned_by_tab_id.as_deref(),
        );
        let launch_session_id = if agent.resume_session_id.is_none() {
            self.agent_registry
                .with_provider(&agent.provider, |p| p.session_id_injection())
                .filter(|mode| !matches!(mode, crate::agent::provider::SessionIdInjection::None))
                .map(|_| uuid::Uuid::new_v4().to_string())
        } else {
            None
        };
        if let Some(ref sid) = agent.resume_session_id {
            self.set_agent_session_id(&tab_id, Some(sid.clone()));
        } else if let Some(ref sid) = launch_session_id {
            self.set_agent_session_id(&tab_id, Some(sid.clone()));
        }

        let mut env_vars: HashMap<String, String> = HashMap::new();
        env_vars.insert("CODETERM_TAB_ID".into(), tab_id.clone());
        let agent_pad = self.ensure_agent_pad(&tab_id).inspect_err(|_| {
            self.rollback_provisional_tab(&ws_id, &tab_id);
        })?;
        env_vars.insert("CODETERM_PAD_DIR".into(), agent_pad.env_value());
        env_vars.insert(
            "CODETERM_SESSION_BINDING_NONCE".into(),
            self.ensure_session_binding_nonce(&tab_id),
        );
        if let Some(ref wid) = agent.worker_id {
            env_vars.insert("CODETERM_WORKER_ID".into(), wid.clone());
        }
        env_vars.insert(
            "CODETERM_SOCKET_PATH".into(),
            crate::socket::server::socket_display_path(),
        );
        let api_cfg = crate::config::load_config().api;
        let (api_url, api_port) = crate::config::api_callback_url(&api_cfg);
        env_vars.insert("CODETERM_API_URL".into(), api_url);
        env_vars.insert("CODETERM_API_PORT".into(), api_port);
        match api_cfg.token {
            Some(ref tok) if !tok.is_empty() => {
                log::debug!(
                    "[spawn-token] tab={} injecting CODETERM_API_TOKEN {}",
                    tab_id,
                    crate::api::auth::token_fp(tok)
                );
                env_vars.insert("CODETERM_API_TOKEN".into(), tok.clone());
            }
            _ => {
                log::warn!(
                    "[spawn-token] tab={} NO api token from load_config — CODETERM_API_TOKEN unset",
                    tab_id
                );
            }
        }
        for (k, v) in &agent.extra_env {
            env_vars.insert(k.clone(), v.clone());
        }

        // Hidden agents now launch as ordinary interactive claude (no -p/stream-json), so use the same 120x40 PTY a visible agent gets.
        let pty_id = {
            let mut pty = self.pty.lock();
            match pty.spawn(None, resolved_cwd, 120, 40, env_vars) {
                Ok((id, _)) => id,
                Err(error) => {
                    drop(pty);
                    let _ = self.revoke_agent_pad(&tab_id);
                    self.rollback_provisional_tab(&ws_id, &tab_id);
                    return Err(error);
                }
            }
        };

        let orphan = self.mux.write().link_pty(&tab_id, pty_id.clone());
        if let Some(orphan) = orphan {
            self.kill_ptys(&[orphan]);
        }
        crate::service::orchestrator::tab_relink_signal::notify_tab_relinked();
        self.agent_run_pty_map
            .lock()
            .insert(tab_id.clone(), pty_id.clone());
        let assignment = agent.task.as_ref().map(|task| {
            if let Some(ref preamble) = agent.task_preamble {
                format!("{preamble}{task}")
            } else {
                task.clone()
            }
        });
        let wrapped_task = crate::service::orchestrator::provider_launch_task(
            &self.agent_registry,
            &agent.provider,
            assignment.as_deref(),
            agent.role.as_deref(),
            agent.role_profile.as_deref(),
        );
        // later (after the cron registry links tab→run).
        if let Some(ref worker_id) = agent.worker_id {
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
                tab_id.clone(),
                crate::agent::run_watcher::AgentRunKind::MemWorker {
                    worker_id: worker_id.clone(),
                },
                timeout,
                wrapped_task.clone(),
            );
        }
        self.emit_state_changed();

        let mut model_args: Vec<String> = launch_model
            .as_ref()
            .and_then(|m| {
                self.agent_registry
                    .with_provider(&agent.provider, |p| p.model_launch_args(m))
            })
            .unwrap_or_default();
        if let Some(ref effort) = agent.reasoning_effort {
            if !effort.is_empty() {
                let effort_args = self
                    .agent_registry
                    .with_provider(&agent.provider, |p| p.reasoning_effort_launch_args(effort))
                    .unwrap_or_default();
                model_args.extend(effort_args);
            }
        }

        let svc_outer = std::sync::Arc::new(self.clone());
        let tab_id_outer = tab_id.clone();
        let pty_id_outer = pty_id.clone();
        let provider = agent.provider.clone();
        let launch_session_id = launch_session_id.clone();
        let task_for_thread = wrapped_task.clone();
        let role_for_thread = agent.role.clone();
        let role_profile_for_thread = agent.role_profile.clone();
        // PtyInput providers (codex/opencode) don't embed the task in the launch command — it must be typed into the TUI after boot. InlineFlag (claude) bakes it in, so PtyInput delivery is skipped there.
        let task_delivery = self
            .agent_registry
            .with_provider(&provider, |p| p.task_delivery())
            .unwrap_or(crate::agent::provider::TaskDelivery::PtyInput);
        let pty_input_task = match task_delivery {
            crate::agent::provider::TaskDelivery::PtyInput => wrapped_task.clone(),
            crate::agent::provider::TaskDelivery::InlineFlag => None,
        };
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

            // from the standard agent rails, not a stream-json result event.
            let hidden_cwd = svc_outer
                .mux
                .read()
                .get_tab(&tab_id_outer)
                .and_then(|p| p.cwd.clone())
                .unwrap_or_default();
            let mut launch_params = crate::agent::provider::AgentLaunchParams {
                agent_name: None,
                task: task_for_thread.clone(),
                starter_prompt: Default::default(),
                args: if model_args.is_empty() {
                    None
                } else {
                    Some(model_args)
                },
                skip_permissions: permission_bypass,
                session_id: launch_session_id,
                structured_prompt_server: crate::service::structured_prompt::ensure_for_spawn(
                    &svc_outer,
                    &provider,
                    &tab_id_outer,
                    &hidden_cwd,
                ),
                launch_marker: None,
            };
            let launch_cmd = super::launch_session_detection::marked_provider_launch_command(
                &svc_outer.agent_registry,
                &provider,
                &tab_id_outer,
                &mut launch_params,
            )
            .unwrap_or_else(|| provider.clone());
            crate::service::launch_diagnostics::log_launch(
                crate::service::launch_diagnostics::AgentLaunchOrigin::HiddenSpawn,
                &tab_id_outer,
                &provider,
                &launch_cmd,
                task_for_thread.as_deref(),
                role_for_thread.as_deref(),
                role_profile_for_thread.as_deref(),
                "provider_descriptor",
            );

            match svc_outer.resolve_pty_id(&tab_id_outer) {
                Ok(pty_id) => {
                    if let Err(e) = svc_outer.send_shell_spawn(&pty_id, &launch_cmd) {
                        log::error!("[spawn.hidden] shell spawn failed: {}", e);
                    } else if let Some(ref task_text) = pty_input_task {
                        if let Err(e) = deliver_hidden_pty_task(
                            &svc_outer,
                            &pty_id,
                            &tab_id_outer,
                            &provider,
                            task_text,
                        ) {
                            log::error!(
                                "[spawn.hidden] task delivery failed for Tab {}: {}",
                                tab_id_outer,
                                e
                            );
                            svc_outer.run_watcher.fail_now(
                                &svc_outer,
                                &pty_id,
                                format!("task delivery failed: {e}"),
                            );
                        } else {
                            svc_outer.run_watcher.arm(&pty_id);
                        }
                    } else {
                        svc_outer.run_watcher.arm(&pty_id);
                    }
                }
                Err(e) => log::error!("[spawn.hidden] no pty for Tab {}: {}", tab_id_outer, e),
            }
            log::debug!(
                "[spawn.hidden] agent launched tab={} provider={}",
                tab_id_outer,
                provider
            );
        });

        let budget_notice = self.workspace_tab_budget_notice(&ws_id);
        Ok(TabHandle {
            tab_id,
            pty_id: Some(pty_id),
            workspace_id: ws_id,
            description: spec.description,
            worker_id: agent.worker_id.clone(),
            warning: cli_warning,
            model_fallback: model_resolution.fallback_notice,
            budget_notice,
        })
    }
}

/// A visible launch carries a minted marker so a hook-less provider still binds
/// its own session, and lane setup runs before it.
fn visible_agent_launch_command(
    tab_id: &str,
    agent_cmd: &str,
    params: &super::SpawnAgentParams,
    task: Option<&str>,
    injected_session_id: Option<&str>,
    lane_setup_command: Option<&str>,
    registry: &AgentRegistry,
) -> String {
    super::worker_lane::compose_launch_with_setup(
        lane_setup_command,
        super::launch_session_detection::marked_agent_command(
            tab_id,
            agent_cmd,
            params,
            task,
            injected_session_id,
            registry,
        ),
    )
}

#[cfg(test)]
#[path = "spawn_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "spawn_tests_chatbackend.rs"]
mod tests_chatbackend;

#[cfg(test)]
#[path = "spawn_launch_marker_tests.rs"]
mod launch_marker_tests;
