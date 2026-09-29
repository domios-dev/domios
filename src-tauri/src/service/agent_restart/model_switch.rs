//! Live model switch for one tab. The provider's capability says how: a
//! `RestartResume` provider is relaunched on its own verified session with the
//! new model launch args. A launch report or first post-relaunch turn confirms
//! the model; a ready relaunch without either returns pending confirmation.

use std::time::{Duration, Instant};

use super::{
    AgentRestartError, RestartAttemptError, RestartOutcome, TabRestartSnapshot, DEFAULT_GRACE_SECS,
};
use crate::agent::provider::ModelSwitchMode;
use crate::multiplexer::types::RestorableLaunchParameters;
use crate::service::{ServiceContext, SpawnAgentParams};

/// Deadline for the relaunched session to report its model before returning pending.
pub(crate) const MODEL_OBSERVE_TIMEOUT_SECS: u64 = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModelSwitchEvidence {
    SessionStartReport,
    FirstPostRelaunchTurn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModelSwitchConfirmation {
    Observed {
        model: String,
        evidence: ModelSwitchEvidence,
    },
    Pending,
}

#[derive(Debug, Clone)]
pub(crate) struct ModelSwitchOutcome {
    pub restart: RestartOutcome,
    pub mechanism: ModelSwitchMode,
    pub previous_model: Option<String>,
    pub requested_model: String,
    pub launched_model: String,
    pub confirmation: ModelSwitchConfirmation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelSwitchObservation {
    model: String,
    evidence: ModelSwitchEvidence,
}

#[derive(Debug)]
pub(crate) enum ModelSwitchError {
    NotFound(String),
    Unsupported {
        provider: String,
    },
    InvalidModel(crate::service::spawn::InvalidModelError),
    NotSwitchable(String),
    RateLimited {
        retry_after_secs: u64,
    },
    RelaunchFailed {
        outcome: RestartOutcome,
        reason: String,
        timed_out: bool,
    },
    ModelMismatch {
        outcome: RestartOutcome,
        launched_model: String,
        observed_model: String,
    },
}

impl std::fmt::Display for ModelSwitchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(message) | Self::NotSwitchable(message) => formatter.write_str(message),
            Self::Unsupported { provider } => write!(
                formatter,
                "provider {provider} cannot switch a live agent's model; respawn with `codeterm agent spawn {provider} --model MODEL`"
            ),
            Self::InvalidModel(error) => write!(formatter, "{error}"),
            Self::RateLimited { retry_after_secs } => write!(
                formatter,
                "this Tab restarted moments ago; retry the model switch in {retry_after_secs} seconds"
            ),
            Self::RelaunchFailed { reason, .. } => {
                write!(formatter, "model switch relaunch failed: {reason}")
            }
            Self::ModelMismatch {
                launched_model,
                observed_model: observed,
                ..
            } => write!(
                formatter,
                "relaunched with {launched_model}, but the current session reported {observed}"
            ),
        }
    }
}

impl From<RestartAttemptError> for ModelSwitchError {
    fn from(error: RestartAttemptError) -> Self {
        match error {
            RestartAttemptError::Request(error) => error.into(),
            RestartAttemptError::Failed {
                outcome,
                reason,
                timed_out,
            } => Self::RelaunchFailed {
                outcome,
                reason,
                timed_out,
            },
        }
    }
}

impl From<AgentRestartError> for ModelSwitchError {
    fn from(error: AgentRestartError) -> Self {
        match error {
            AgentRestartError::NotFound(message) => Self::NotFound(message),
            AgentRestartError::BadRequest(message) | AgentRestartError::Internal(message) => {
                Self::NotSwitchable(message)
            }
            AgentRestartError::RateLimited { retry_after_secs } => {
                Self::RateLimited { retry_after_secs }
            }
        }
    }
}

/// The relaunch a model switch performs: the same parameters a daemon restore
/// would use for this tab, with only the model replaced.
pub(super) struct ModelSwitchLaunch {
    provider: String,
    session_id: String,
    params: SpawnAgentParams,
    launch: RestorableLaunchParameters,
    task: Option<String>,
}

impl ModelSwitchLaunch {
    fn prepare(
        svc: &ServiceContext,
        snap: &TabRestartSnapshot,
        session_id: &str,
        requested: &str,
    ) -> Result<Self, ModelSwitchError> {
        let registry = &svc.agent_registry;
        let resolution = crate::service::spawn::resolve_fresh_agent_model_with_notice(
            registry,
            &snap.provider,
            Some(requested.to_string()),
            Some(session_id),
        );
        let model = resolution.model.clone().ok_or_else(|| {
            ModelSwitchError::NotSwitchable(format!(
                "provider {} resolved no launch model for '{requested}'",
                snap.provider
            ))
        })?;
        let mut launch = {
            let mux = svc.mux.read();
            let tab = mux.get_tab(&snap.tab_id).ok_or_else(|| {
                ModelSwitchError::NotFound(format!("Tab {} not found", snap.tab_id))
            })?;
            recorded_launch_parameters(tab)
        };
        launch.requested_model = resolution
            .requested_model
            .clone()
            .or_else(|| Some(requested.to_string()));
        launch.model = Some(model.clone());
        let authorization =
            crate::service::restored_launch::restore_permission_bypass_authorization(
                registry,
                &snap.tab_id,
                &snap.provider,
                &launch,
            )
            .map_err(ModelSwitchError::NotSwitchable)?;
        let skip_permissions = if launch.captured {
            launch.permission_bypass_requested && authorization.is_some()
        } else {
            authorization.is_some()
        };
        let reasoning_effort = launch
            .resolved_reasoning_effort
            .clone()
            .or_else(|| launch.requested_reasoning_effort.clone());
        let (prompt_capable, task) = crate::service::restored_launch::restored_launch_task(
            registry,
            &snap.provider,
            snap.role.as_deref(),
            snap.role_profile.as_deref(),
            &launch,
        );
        let system_prompt = prompt_capable.then(|| task.clone()).flatten();
        if !launch.captured {
            launch.permission_bypass_requested = skip_permissions;
        }
        launch.captured = true;
        launch.identity_instructions = system_prompt.clone();
        launch.resolved_reasoning_effort = reasoning_effort.clone();
        let params = SpawnAgentParams {
            agent: Some(snap.provider.clone()),
            agent_name: launch.agent_name.clone(),
            args: launch.args.clone(),
            cwd: snap.cwd.clone(),
            skip_permissions: Some(skip_permissions),
            permission_bypass_authorization: authorization,
            model: Some(model),
            reasoning_effort,
            system_prompt,
            resume_session_id: Some(session_id.to_string()),
            role: snap.role.clone(),
            role_profile: snap.role_profile.clone(),
            starter_prompt: crate::agent::provider::StarterPrompt::NoStarter,
            ..Default::default()
        };
        Ok(Self {
            provider: snap.provider.clone(),
            session_id: session_id.to_string(),
            params,
            launch,
            task,
        })
    }

    fn launched_model(&self) -> &str {
        self.params.model.as_deref().unwrap_or_default()
    }

    pub(super) fn command(
        &self,
        svc: &ServiceContext,
        snap: &TabRestartSnapshot,
    ) -> Option<(String, Option<String>)> {
        let command = crate::service::orchestrator::build_agent_command_for_service(
            svc,
            &snap.tab_id,
            &self.provider,
            &self.params,
            self.task.as_deref(),
            None,
            &svc.agent_registry,
        );
        (!command.trim().is_empty()).then(|| (command, self.task.clone()))
    }

    /// The tab's spawn record now names the new model, so a daemon restore
    /// relaunches what is actually running.
    pub(super) fn commit(&self, svc: &ServiceContext, tab_id: &str) {
        crate::service::restored_launch::persist_restored_launch_identity(
            &svc.mux,
            tab_id,
            &self.provider,
            &self.params,
            &self.launch,
        );
        if let Some(tab) = svc.mux.write().get_tab_mut(tab_id) {
            tab.model_fallback = None;
            tab.pending_model_switch_observation = Some(
                crate::multiplexer::types::PendingModelSwitchObservation {
                    session_id: self.session_id.clone(),
                    launched_model: self.launched_model().to_string(),
                },
            );
        }
        if let Err(error) = crate::service::session_persist::persist_session_after_mutation(
            svc,
            crate::service::session_persist::SessionPersistUrgency::Prompt,
        ) {
            log::warn!("[agent-model] tab={tab_id} failed to persist the switched model: {error}");
        }
        svc.emit_state_changed();
    }
}

/// The restorable launch record with the same fallbacks a restore applies.
fn recorded_launch_parameters(tab: &crate::multiplexer::types::Tab) -> RestorableLaunchParameters {
    let mut launch = tab.restorable_launch.clone();
    launch.requested_model = launch
        .requested_model
        .or_else(|| tab.agent_requested_model.clone());
    launch.model = launch
        .model
        .or_else(|| tab.agent_provider_resolved_model.clone())
        .or_else(|| tab.agent_session_model.clone());
    launch.requested_reasoning_effort = launch
        .requested_reasoning_effort
        .or_else(|| tab.reasoning_effort_identity.requested.clone());
    launch.resolved_reasoning_effort = launch
        .resolved_reasoning_effort
        .or_else(|| tab.reasoning_effort_identity.resolved.clone());
    launch
}

impl ServiceContext {
    pub(crate) fn agent_switch_model(
        &self,
        tab_id: &str,
        requested: &str,
    ) -> Result<ModelSwitchOutcome, ModelSwitchError> {
        let requested = requested.trim();
        let (resolved_tab, provider) = {
            let mux = self.mux.read();
            let resolved = mux
                .resolve_tab_id(tab_id)
                .ok_or_else(|| ModelSwitchError::NotFound(format!("Tab {tab_id} not found")))?;
            let provider = mux
                .get_tab(&resolved)
                .and_then(|tab| tab.agent_type.clone())
                .ok_or_else(|| {
                    ModelSwitchError::NotSwitchable("no agent in this Tab".to_string())
                })?;
            (resolved, provider)
        };
        let mechanism = self
            .agent_registry
            .with_provider(&provider, |p| p.model_switch())
            .ok_or_else(|| {
                ModelSwitchError::NotSwitchable(format!("unknown agent provider: {provider}"))
            })?;
        if mechanism == ModelSwitchMode::Unsupported {
            return Err(ModelSwitchError::Unsupported { provider });
        }
        if self.general_agent_restart_snapshot(&resolved_tab).is_some() {
            return Err(ModelSwitchError::NotSwitchable(
                "the General Agent's model comes from its configuration; change it in Settings"
                    .to_string(),
            ));
        }
        crate::service::spawn::validate_agent_model(
            &self.agent_registry,
            &provider,
            Some(requested),
        )
        .map_err(ModelSwitchError::InvalidModel)?;
        let snap = self.snapshot_tab_for_restart(&resolved_tab)?;
        let session_id = self.verified_resumable_session(&snap)?;
        if !self.tab_is_idle(&snap.tab_id) {
            return Err(ModelSwitchError::NotSwitchable(
                "the agent is mid-turn or waiting on a prompt; switch its model once it is idle"
                    .to_string(),
            ));
        }
        let previous_model = {
            let mux = self.mux.read();
            mux.get_tab(&snap.tab_id).and_then(|tab| {
                tab.agent_session_model
                    .clone()
                    .or_else(|| tab.agent_provider_resolved_model.clone())
            })
        };
        let launch = ModelSwitchLaunch::prepare(self, &snap, &session_id, requested)?;
        let launched_model = launch.launched_model().to_string();
        log::info!(
            "[agent-model] tab={} provider={} session={} {:?} -> {}",
            snap.tab_id,
            provider,
            session_id,
            previous_model,
            launched_model
        );
        let restart = self.restart_live_agent(snap, false, DEFAULT_GRACE_SECS, Some(&launch))?;
        let observed = self.await_model_switch_observation(
            &restart.tab_id,
            &provider,
            &session_id,
            &launched_model,
            Duration::from_secs(MODEL_OBSERVE_TIMEOUT_SECS),
        );
        let confirmation = match model_switch_confirmation(
            &self.agent_registry,
            &provider,
            &launched_model,
            observed,
        ) {
            Ok(confirmation) => confirmation,
            Err(observed_model) => {
                return Err(ModelSwitchError::ModelMismatch {
                    outcome: restart,
                    launched_model,
                    observed_model,
                })
            }
        };
        Ok(ModelSwitchOutcome {
            restart,
            mechanism,
            previous_model,
            requested_model: requested.to_string(),
            launched_model,
            confirmation,
        })
    }

    fn verified_resumable_session(
        &self,
        snap: &TabRestartSnapshot,
    ) -> Result<String, ModelSwitchError> {
        let verified = {
            let mux = self.mux.read();
            mux.get_tab(&snap.tab_id).is_some_and(|tab| {
                tab.session_binding_nonce.is_some()
                    && tab.session_binding_nonce == tab.reported_session_binding_nonce
            })
        };
        match snap.session_id.clone() {
            Some(session_id) if verified && self.session_file_present(snap) => Ok(session_id),
            _ => Err(ModelSwitchError::NotSwitchable(
                "the Tab has no verified resumable session to carry into the new model; spawn a fresh agent with --model"
                    .to_string(),
            )),
        }
    }

    fn await_model_switch_observation(
        &self,
        tab_id: &str,
        provider: &str,
        session_id: &str,
        launched_model: &str,
        timeout: Duration,
    ) -> Option<ModelSwitchObservation> {
        let mut observed = None;
        let mut mismatched_report = None;
        crate::service::boot_wait::wait_until(Instant::now() + timeout, || {
            let (report, model, pending_model) = {
                let mux = self.mux.read();
                let Some(tab) = mux.get_tab(tab_id) else {
                    return false;
                };
                let identity_current = crate::service::session_model::has_current_session_model(
                    tab, session_id,
                );
                (
                    crate::service::session_model::current_launch_report(tab, session_id),
                    identity_current.then(|| tab.agent_session_model.clone()).flatten(),
                    tab.pending_model_switch_observation
                        .as_ref()
                        .filter(|pending| pending.session_id == session_id)
                        .map(|pending| pending.launched_model.clone()),
                )
            };
            if let Some(report) = report {
                if crate::service::session_model::session_model_matches(
                    &self.agent_registry,
                    provider,
                    pending_model.as_deref().unwrap_or(launched_model),
                    &report.model,
                ) {
                    observed = Some(ModelSwitchObservation {
                        model: report.model,
                        evidence: ModelSwitchEvidence::SessionStartReport,
                    });
                    return true;
                }
                mismatched_report = Some(ModelSwitchObservation {
                    model: report.model,
                    evidence: ModelSwitchEvidence::SessionStartReport,
                });
            }
            if pending_model.is_none() {
                if let Some(model) = model {
                    observed = Some(ModelSwitchObservation {
                        model,
                        evidence: ModelSwitchEvidence::FirstPostRelaunchTurn,
                    });
                    return true;
                }
                false
            } else {
                false
            }
        });
        observed.or(mismatched_report)
    }
}

fn model_switch_confirmation(
    registry: &crate::agent::registry::AgentRegistry,
    provider: &str,
    launched_model: &str,
    observation: Option<ModelSwitchObservation>,
) -> Result<ModelSwitchConfirmation, String> {
    let Some(observation) = observation else {
        return Ok(ModelSwitchConfirmation::Pending);
    };
    if crate::service::session_model::session_model_matches(
        registry,
        provider,
        launched_model,
        &observation.model,
    ) {
        Ok(ModelSwitchConfirmation::Observed {
            model: observation.model,
            evidence: observation.evidence,
        })
    } else {
        Err(observation.model)
    }
}

#[cfg(test)]
mod tests;
