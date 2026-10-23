use super::{RestartOutcome, TabRestartSnapshot};
use crate::agent::provider::ProviderTransport;
use crate::multiplexer::types::{
    ContextEpochTransport, RestorableLaunchParameters, Tab, TabBacking,
};
use crate::service::{agent_runtime, provider_handoff, ServiceContext, SpawnAgentParams};

mod general;
mod input;
mod model;
mod operations;
use model::{HandoffRoute, TargetRoute};
pub use operations::{
    AcceptedProviderSwitch, ProviderSwitchOperations, ProviderSwitchRequest, ProviderSwitchStatus,
};

#[cfg(test)]
mod tests;

#[derive(Clone, serde::Serialize)]
pub struct ProviderSwitchOutcome {
    pub tab_id: String,
    pub provider: String,
    pub session_id: String,
    pub previous_provider: String,
    pub previous_session_id: Option<String>,
    pub previous_history_path: String,
    pub binding_restored: bool,
    pub handoff_delivered: bool,
}

struct SwitchBindingCleanup<'a> {
    service: &'a ServiceContext,
    tab_id: &'a str,
    interrupt_on_failure: bool,
}

impl Drop for SwitchBindingCleanup<'_> {
    fn drop(&mut self) {
        self.service.clear_runtime_restart_binding(self.tab_id);
        if self.interrupt_on_failure {
            self.service.signal_worker_interruption(
                self.tab_id,
                crate::service::worker_interruption::WorkerInterruptionEvent::ForcedContextReset,
            );
        }
    }
}

fn prepare_handoff(handoff: &str) -> Result<crate::service::messenger::AgentInput, String> {
    crate::service::messenger::Messenger::restore_framed_delivery(handoff)
        .map(crate::service::messenger::AgentInput::Framed)
        .map_err(|error| format!("provider_switch_handoff_invalid: {error}"))
}

impl ServiceContext {
    pub fn switch_provider_in_place(
        &self,
        tab_id: &str,
        target: &str,
        requested_model: Option<String>,
        effort: Option<String>,
        force: bool,
    ) -> Result<ProviderSwitchOutcome, String> {
        if let Some(refusal) = self.tab_write_lock(tab_id) {
            return Err(refusal.to_string());
        }
        let mut tab = {
            let mux = self.mux.read();
            let id = mux
                .resolve_tab_id(tab_id)
                .ok_or("provider_switch_not_found: tab not found")?;
            mux.get_tab(&id)
                .cloned()
                .ok_or("provider_switch_not_found: tab not found")?
        };
        let source = tab
            .agent_type
            .as_deref()
            .ok_or("provider_switch_no_agent: tab has no provider")?;
        let route = self.provider_switch_route(source, target, &tab)?;
        let _general_claim = self.claim_general_provider_switch(&tab)?;
        model::require_idle(
            self.provider_switch_idle(&tab.id, route.source),
            force,
            tab.agent_booting,
        )?;
        self.require_background_work_safe(&tab.id, force)
            .map_err(|error| format!("provider_switch_background_work: {error:?}"))?;
        crate::service::spawn::validate_agent_model(
            &self.agent_registry,
            target,
            requested_model.as_deref(),
        )
        .map_err(|error| error.to_string())?;
        let resolved = crate::service::spawn::resolve_fresh_agent_model_with_notice(
            &self.configuration(),
            &self.agent_registry,
            target,
            requested_model.clone(),
            None,
        );
        let model = resolved.model;
        crate::service::spawn::validate_agent_reasoning_effort(
            &self.agent_registry,
            target,
            model.as_deref(),
            effort.as_deref(),
        )?;
        let permission = crate::service::permission_policy::configured_permission_bypass_for(
            self,
            &self.agent_registry,
            target,
        )?;
        crate::service::spawn::provider_launch_preflight(
            &self.agent_registry,
            target,
            permission.is_some(),
        )
        .map_err(switch_preflight_error)?;
        self.provider_switch_auth_preflight(target)?;
        let _claim = self
            .claim_restart(&tab.id, false)
            .map_err(|error| format!("provider_switch_in_progress: {error:?}"))?;
        let nonce = tab
            .session_binding_nonce
            .as_deref()
            .ok_or("provider_switch_binding_missing: source binding unavailable")?;
        let carry = self.authorize_provider_switch_binding(&tab.id, source, target, nonce)?;
        tab.task_binding = self
            .mux
            .read()
            .get_tab(&tab.id)
            .and_then(|tab| tab.task_binding.clone());
        if super::restart_carries_task_attempt(tab.task_binding.as_ref()) && carry.is_none() {
            return Err("provider_switch_binding_refused: task attempt cannot be carried".into());
        }
        if tab.restorable_launch.execution_sandbox.is_some() {
            return Err(
                "provider_switch_confinement: target execution access mapping is unavailable"
                    .into(),
            );
        }
        let pad = self.ensure_agent_pad(&tab.id)?;
        let (messages, _) = crate::service::tab_chat::tab_chat_snapshot(self, &tab.id, 0)?;
        let archive = crate::service::launch_diagnostics::redact_secrets(
            &serde_json::to_string(&messages).map_err(|error| error.to_string())?,
        );
        let archive = pad.write_inbox_body(archive.as_bytes())?;
        let handoff = self.provider_switch_input(&tab, &messages, &archive)?;
        let handoff = prepare_handoff(&handoff)?;
        let full_access = permission.is_some();
        let mut params = SpawnAgentParams {
            agent: Some(target.into()),
            cwd: tab.cwd.clone(),
            model: model.clone(),
            reasoning_effort: effort.clone(),
            role: tab.role.map(|role| role.label().into()),
            role_profile: tab.role_profile.clone(),
            skip_permissions: Some(permission.is_some()),
            permission_bypass_authorization: permission,
            ..Default::default()
        };
        let identity_instructions =
            self.general_provider_switch_prompt(&tab, target)
                .or_else(|| {
                    (route.target == TargetRoute::Runtime)
                        .then(|| {
                            crate::service::orchestrator::compose_runtime_system_prompt(&mut params)
                        })
                        .flatten()
                });
        let input = crate::service::restored_launch::restored_fresh_launch_input(
            &self.agent_registry,
            params,
            RestorableLaunchParameters {
                identity_instructions,
                captured: true,
                requested_model,
                model,
                requested_reasoning_effort: effort.clone(),
                resolved_reasoning_effort: effort,
                permission_bypass_requested: full_access,
                ..Default::default()
            },
        );
        let prepared =
            if route.target == TargetRoute::Pty {
                Some(input.prepared_command(self, &tab.id).ok_or(
                    "provider_switch_launch_unavailable: target returned no launch command",
                )?)
            } else {
                None
            };
        let snap = TabRestartSnapshot {
            tab_id: tab.id.clone(),
            pty_id: tab.pty_id.clone().unwrap_or_default(),
            provider: source.into(),
            target_provider: Some(target.into()),
            session_id: tab.agent_session_id.clone(),
            cwd: tab.cwd.clone(),
            auto_approve: input.params.permission_bypass_requested(target),
            role: input.params.role.clone(),
            role_profile: tab.role_profile.clone(),
            handoff_history: None,
        };
        {
            let mux = self.mux.read();
            let current = mux
                .get_tab(&tab.id)
                .ok_or("provider_switch_superseded: tab closed")?;
            if current.agent_type != tab.agent_type
                || current.agent_session_id != tab.agent_session_id
                || current.session_binding_nonce != tab.session_binding_nonce
            {
                return Err("provider_switch_superseded: source binding changed".into());
            }
        }
        model::require_idle(
            self.provider_switch_idle(&tab.id, route.source),
            force,
            false,
        )?;
        self.retire_provider_switch_input(&tab.id)?;
        crate::service::orchestrator::supersede_tab_launch(&tab.id);
        let carry_required = carry.is_some();
        self.stage_runtime_restart_binding(&tab.id, carry);
        let mut binding_cleanup = SwitchBindingCleanup {
            service: self,
            tab_id: &tab.id,
            interrupt_on_failure: carry_required,
        };
        match route.source {
            TargetRoute::Runtime => {
                agent_runtime::engine::stop_blocking(self, &tab.id)?;
                self.clear_tab_agent_identity_after_confirmed_exit(
                    &tab.id,
                    "provider switch retired the source runtime",
                );
            }
            TargetRoute::Pty => {
                let _pty_claim = self.pty_ownership.acquire(&tab.id);
                self.request_graceful_exit(
                    &snap,
                    force,
                    std::time::Duration::from_secs(super::DEFAULT_GRACE_SECS),
                );
                self.tear_down_restart_pty(&snap)?;
            }
        }
        self.archive_provider_switch_source(&tab.id, &tab)?;
        {
            let mut mux = self.mux.write();
            let current = mux
                .get_tab_mut(&tab.id)
                .ok_or("provider_switch_superseded: tab closed after retirement")?;
            model::reset_for_target(current, target, route.target, &input.launch_parameters);
        }
        self.set_and_lock_tab_agent_type_from(
            &tab.id,
            target.into(),
            crate::session::lifecycle::AgentTypeWriteOrigin::Spawn,
            None,
        );
        if crate::service::general_agent::is_general_agent(&tab) {
            crate::service::general_agent::generate_instructions(
                self.configuration().general_agent.capability_hints,
                target,
                Some(&self.mem),
            );
        }
        let result = self.provider_switch_launch(
            &snap,
            route.target,
            &input,
            prepared,
            carry_required,
            force,
        );
        if result.is_err() {
            self.clear_runtime_restart_binding(&tab.id);
            if let Err(reason) = &result {
                log::debug!(
                    "[provider-switch] tab={} target={} launch failed: {}",
                    tab.id,
                    target,
                    reason
                );
                let mut mux = self.mux.write();
                if let Some(current) = mux.get_tab_mut(&tab.id) {
                    current.agent_booting = false;
                    if !agent_runtime::engine::has_live_bound_session(current) {
                        crate::service::boot_stage::apply_boot_stage(
                            current,
                            codeterm_types::pty::BootStage::SpawnFailed,
                            Some(reason),
                        );
                    }
                }
            }
            self.schedule_session_persist_debounced();
            self.emit_state_changed(crate::service::MuxEmitCause::AgentRestart);
        }
        let session_id = result?;
        binding_cleanup.interrupt_on_failure = false;
        self.persist_general_provider_switch(&tab, target, &input.launch_parameters, &session_id)?;
        self.send_text(&tab.id, &handoff).map_err(|error| format!("provider_switch_handoff_unconfirmed: target session {session_id} is running; input will not be replayed: {error}"))?;
        self.schedule_session_persist_debounced();
        self.emit_state_changed(crate::service::MuxEmitCause::AgentRestart);
        Ok(ProviderSwitchOutcome {
            tab_id: tab.id.clone(),
            provider: target.into(),
            session_id,
            previous_provider: source.into(),
            previous_session_id: tab.agent_session_id,
            previous_history_path: archive.display().to_string(),
            binding_restored: true,
            handoff_delivered: true,
        })
    }

    fn retire_provider_switch_input(&self, tab: &str) -> Result<(), String> {
        self.end_held_user_sends_for_fresh_restart(tab)?;
        self.discard_pending_delivery(tab)
            .map_err(|error| error.to_string())
    }

    fn archive_provider_switch_source(&self, tab_id: &str, source: &Tab) -> Result<(), String> {
        let (provider, session_id, transport) = match &source.backing {
            TabBacking::Pty => (
                source.agent_type.clone(),
                source.agent_session_id.clone(),
                ContextEpochTransport::AgentProvider,
            ),
            TabBacking::ChatBackend {
                plugin_id,
                session_id,
            } => (
                Some(plugin_id.clone()),
                Some(session_id.clone())
                    .filter(|id| !id.is_empty())
                    .or_else(|| source.agent_session_id.clone()),
                ContextEpochTransport::ChatBackend,
            ),
            TabBacking::Surface(_) => (None, None, ContextEpochTransport::AgentProvider),
        };
        let (Some(provider), Some(session_id)) = (provider, session_id) else {
            return Ok(());
        };
        let mut mux = self.mux.write();
        let tab = mux
            .get_tab_mut(tab_id)
            .ok_or("provider_switch_superseded: tab closed after source retirement")?;
        tab.archive_provider_handoff(
            &provider,
            source.cwd.as_deref().unwrap_or_default(),
            &session_id,
            transport,
        );
        Ok(())
    }

    fn provider_switch_idle(&self, tab_id: &str, route: TargetRoute) -> bool {
        let tab = self.mux.read().get_tab(tab_id).cloned();
        let Some(tab) = tab else {
            return false;
        };
        if model::sleeping_source_idle(&tab) {
            return true;
        }
        if route == TargetRoute::Pty {
            return self.tab_is_idle(tab_id);
        }
        let Some(session) = agent_runtime::engine::for_tab(tab_id) else {
            return false;
        };
        let live = agent_runtime::engine::has_live_bound_session(&tab);
        let context = session.context.lock().clone();
        let active = context.active_turn_id.is_some() || !context.active_turns.is_empty();
        let prompts = !session.prompts.lock().is_empty();
        model::runtime_idle(live, active, prompts)
    }

    fn provider_switch_launch(
        &self,
        snap: &TabRestartSnapshot,
        route: TargetRoute,
        input: &crate::service::restored_launch::RestoredFreshLaunchInput,
        prepared: Option<crate::service::orchestrator::PreparedAgentCommand>,
        carry_required: bool,
        force: bool,
    ) -> Result<String, String> {
        let target = snap
            .target_provider
            .as_deref()
            .ok_or("provider switch target absent")?;
        let mut outcome = RestartOutcome::accepted(snap, false, force);
        match route {
            TargetRoute::Runtime => {
                self.ensure_session_binding_nonce(&snap.tab_id);
                if let Some(prompt) = input.launch_parameters.identity_instructions.as_ref() {
                    if let Some(tab) = self.mux.write().get_tab_mut(&snap.tab_id) {
                        tab.agent_runtime_config
                            .get_or_insert_with(|| serde_json::json!({}))["systemPrompt"] =
                            serde_json::json!(prompt);
                    }
                }
                self.prepare_agent_runtime_launch_options(&snap.tab_id, &input.params)?;
                self.prepare_agent_runtime_launch_env(&snap.tab_id, &Default::default())?;
                let config = self
                    .mux
                    .read()
                    .get_tab(&snap.tab_id)
                    .and_then(|tab| tab.agent_runtime_config.clone())
                    .unwrap_or_default();
                agent_runtime::transport::launch(self, target, &snap.tab_id, config)?;
                outcome.agent_ready = true;
            }
            TargetRoute::Pty => {
                crate::service::orchestrator::launch_agent_in_background_with_env(
                    self.clone(),
                    snap.tab_id.clone(),
                    prepared.ok_or("target command unavailable")?,
                    target.into(),
                    crate::service::launch_diagnostics::AgentLaunchOrigin::Restart,
                    false,
                    None,
                    input.launch_task.clone(),
                    None,
                    Default::default(),
                    None,
                    None,
                    None,
                );
                outcome = self
                    .finish_observed_restart(
                        outcome,
                        (!snap.pty_id.is_empty()).then_some(snap.pty_id.as_str()),
                        std::time::Duration::from_secs(super::RESTART_READY_TIMEOUT_SECS),
                    )
                    .map_err(|error| format!("provider_switch_launch_failed: {error:?}"))?;
            }
        }
        let outcome = self
            .settle_restored_binding(outcome, snap, carry_required)
            .map_err(|error| format!("provider_switch_binding_failed: {error:?}"))?;
        self.mux
            .read()
            .get_tab(&outcome.tab_id)
            .and_then(|tab| tab.agent_session_id.clone())
            .ok_or("provider_switch_binding_failed: target session unavailable".into())
    }

    fn provider_switch_route(
        &self,
        source: &str,
        target: &str,
        tab: &Tab,
    ) -> Result<HandoffRoute, String> {
        let describe = |id| {
            self.agent_registry.with_provider(id, |provider| {
                (
                    provider.describe().transport,
                    provider.agent_runtime().is_some(),
                )
            })
        };
        let source = describe(source)
            .ok_or("provider_switch_unavailable: source provider is not installed")?;
        let target_capability = describe(target)
            .ok_or("provider_switch_unavailable: target provider is not installed")?;
        model::route(
            source,
            target_capability,
            tab.agent_type.as_deref() == Some(target),
            &tab.backing,
        )
    }

    fn provider_switch_auth_preflight(&self, target: &str) -> Result<(), String> {
        let probe = self
            .agent_registry
            .with_provider(target, |provider| {
                Some((
                    provider.binary_name().to_owned(),
                    provider.auth_status_probe()?,
                ))
            })
            .flatten();
        if let Some((binary, probe)) = probe {
            let record = crate::service::provider_signals_probe::capture_result(&binary, &probe)
                .map_err(|failure| match failure {
                    crate::service::provider_signals_probe::ProbeFailure::TimedOut =>
                        "provider_switch_auth_timeout: target authentication check timed out; retry the switch",
                    crate::service::provider_signals_probe::ProbeFailure::Unavailable =>
                        "provider_switch_auth_unknown: target authentication could not be verified",
                })?;
            let auth = self
                .agent_registry
                .with_provider(target, |provider| provider.parse_auth_status(&record))
                .flatten()
                .ok_or("provider_switch_auth_unknown: target returned no authentication status")?;
            if !auth.logged_in {
                return Err(
                    "provider_switch_auth_required: sign in to the target provider first".into(),
                );
            }
        }
        Ok(())
    }
}

fn switch_preflight_error(error: crate::service::spawn_cli::ProviderPreflightError) -> String {
    match error {
        crate::service::spawn_cli::ProviderPreflightError::BinaryUnavailable { display_name, binary, .. } =>
            format!("provider_switch_unavailable: {display_name} requires {binary}, which is not installed; install it before switching"),
        other => format!("provider_switch_unavailable: {other}"),
    }
}
