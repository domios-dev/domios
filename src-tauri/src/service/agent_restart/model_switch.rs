//! Live model switch for one tab. The provider's capability says how: a
//! `RestartResume` provider is relaunched on its own verified session with the
//! new model launch args. The first assistant turn after the transcript
//! boundary confirms the model; a ready relaunch without one returns pending.

use std::time::{Duration, Instant};

use super::{
    AgentRestartError, RestartAttemptError, RestartOutcome, TabRestartSnapshot, DEFAULT_GRACE_SECS,
};
use crate::agent::provider::ModelSwitchMode;
use crate::multiplexer::types::RestorableLaunchParameters;
use crate::service::{ServiceContext, SpawnAgentParams};

mod live_command;
mod next_turn;
pub(super) mod staging;

/// Deadline for the relaunched session to report its model before returning pending.
pub(crate) const MODEL_OBSERVE_TIMEOUT_SECS: u64 = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModelSwitchEvidence {
    LiveSessionRecord,
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
    AfterTurn,
}

#[derive(Debug, Clone)]
pub(crate) struct ModelSwitchOutcome {
    pub tab_id: String,
    pub provider: String,
    pub session_id: String,
    pub restart: Option<RestartOutcome>,
    pub mechanism: ModelSwitchMode,
    pub fallback_reason: Option<String>,
    pub previous_model: Option<String>,
    pub requested_model: String,
    pub launched_model: String,
    pub confirmation_detail: Option<String>,
    pub confirmation: ModelSwitchConfirmation,
}

fn effective_model_switch_mode(
    declared: ModelSwitchMode,
    live_result: Option<&crate::service::structured_prompt::LiveModelSwitchResult>,
) -> (ModelSwitchMode, Option<String>) {
    match (declared, live_result) {
        (
            ModelSwitchMode::Live,
            Some(crate::service::structured_prompt::LiveModelSwitchResult::Applied { .. }),
        ) => (ModelSwitchMode::Live, None),
        (
            ModelSwitchMode::Live,
            Some(crate::service::structured_prompt::LiveModelSwitchResult::Unsupported { reason }),
        ) => (ModelSwitchMode::RestartResume, Some(reason.clone())),
        (ModelSwitchMode::NextTurn, _) => (ModelSwitchMode::NextTurn, None),
        (ModelSwitchMode::LiveCommand, _) => (ModelSwitchMode::LiveCommand, None),
        (ModelSwitchMode::RestartResume, _) => (ModelSwitchMode::RestartResume, None),
        (ModelSwitchMode::Unsupported, _) => (ModelSwitchMode::Unsupported, None),
        (ModelSwitchMode::Live, None) => (
            ModelSwitchMode::RestartResume,
            Some("the live switch route was not probed".into()),
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelSwitchObservation {
    model: String,
    evidence: ModelSwitchEvidence,
}

#[derive(Debug)]
pub(crate) enum ModelSwitchError {
    BackgroundWork {
        items: Vec<String>,
    },
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
    LiveSwitchFailed(String),
    LiveCommandFailed(String),
    InputNotReady(String),
}

impl std::fmt::Display for ModelSwitchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BackgroundWork {items}=>write!(formatter,"restart refused while background work is running: {}",items.join(", ")),
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
            Self::LiveSwitchFailed(reason) | Self::LiveCommandFailed(reason) | Self::InputNotReady(reason) => {
                write!(formatter, "live model switch failed: {reason}")
            }
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
            AgentRestartError::BackgroundWork { items } => Self::BackgroundWork { items },
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
    transcript_boundary: codeterm_types::agent::SessionModelTranscriptBoundary,
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
        let cwd = snap.cwd.as_deref().ok_or_else(|| {
            ModelSwitchError::NotSwitchable(
                "the session's working directory is unavailable for model verification".to_string(),
            )
        })?;
        let transcript_boundary = svc
            .agent_registry
            .with_provider(&snap.provider, |provider| {
                provider.session_model_transcript_boundary(cwd, session_id)
            })
            .flatten()
            .unwrap_or_default();
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
                &svc.app_handle,
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
            context_files: launch.context_files.clone(),
            context_skills: launch.context_skills.clone(),
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
            transcript_boundary,
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
        if let Some(tab) = svc.mux.write().get_tab_mut(tab_id) {
            tab.model_fallback = None;
            tab.pending_model_switch_observation =
                Some(crate::multiplexer::types::PendingModelSwitchObservation {
                    session_id: self.session_id.clone(),
                    launched_model: self.launched_model().to_string(),
                    transcript_boundary: self.transcript_boundary.clone(),
                });
        }
        crate::service::restored_launch::persist_restored_launch_identity(
            &svc.mux,
            tab_id,
            &self.provider,
            &self.params,
            &self.launch,
        );
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

fn record_live_request_identity(
    tab: &mut crate::multiplexer::types::Tab,
    model: &str,
    effort: Option<&str>,
) {
    tab.agent_requested_model = Some(model.to_owned());
    tab.agent_provider_resolved_model = Some(model.to_owned());
    tab.restorable_launch.captured = true;
    tab.restorable_launch.requested_model = Some(model.to_owned());
    tab.restorable_launch.model = Some(model.to_owned());
    tab.restorable_launch.requested_reasoning_effort = effort.map(str::to_owned);
    tab.restorable_launch.resolved_reasoning_effort = effort.map(str::to_owned);
    crate::service::reasoning_effort::persist_launch_identity(tab, effort, effort);
}

fn live_model_confirmation_matches(
    registry: &crate::agent::registry::AgentRegistry,
    tab: &crate::multiplexer::types::Tab,
    provider: &str,
    session_id: &str,
    binding_nonce: &str,
    observed_model: &str,
) -> bool {
    tab.agent_type.as_deref() == Some(provider)
        && tab.agent_session_id.as_deref() == Some(session_id)
        && tab.agent_session_model.as_deref().is_some_and(|model| {
            crate::service::session_model::session_model_matches(
                registry,
                provider,
                observed_model,
                model,
            )
        })
        && tab
            .agent_session_model_identity
            .as_ref()
            .is_some_and(|identity| {
                identity.session_id == session_id
                    && identity.session_binding_nonce == binding_nonce
                    && tab.session_binding_nonce.as_deref() == Some(binding_nonce)
                    && tab.reported_session_binding_nonce.as_deref() == Some(binding_nonce)
            })
}

impl ServiceContext {
    #[cfg(test)]
    pub(crate) fn agent_switch_model(
        &self,
        tab_id: &str,
        requested: &str,
    ) -> Result<ModelSwitchOutcome, ModelSwitchError> {
        self.agent_switch_model_with_reasoning(tab_id, requested, None)
    }

    #[cfg(test)]
    pub(crate) fn agent_switch_model_with_reasoning(
        &self,
        tab_id: &str,
        requested: &str,
        requested_effort: Option<&str>,
    ) -> Result<ModelSwitchOutcome, ModelSwitchError> {
        self.agent_switch_model_with_options(tab_id, requested, requested_effort, false)
    }

    pub(crate) fn agent_switch_model_with_options(
        &self,
        tab_id: &str,
        requested: &str,
        requested_effort: Option<&str>,
        force: bool,
    ) -> Result<ModelSwitchOutcome, ModelSwitchError> {
        self.agent_switch_model_with_fallback(tab_id, requested, requested_effort, force, false)
    }

    pub(crate) fn agent_switch_model_with_fallback(
        &self,
        tab_id: &str,
        requested: &str,
        requested_effort: Option<&str>,
        force: bool,
        restart_fallback: bool,
    ) -> Result<ModelSwitchOutcome, ModelSwitchError> {
        self.execute_model_switch(
            tab_id,
            requested,
            requested_effort,
            force,
            restart_fallback,
            None,
        )
    }

    fn execute_model_switch(
        &self,
        tab_id: &str,
        requested: &str,
        requested_effort: Option<&str>,
        force: bool,
        restart_fallback: bool,
        staged: Option<&staging::SwitchRequest>,
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
        let completion_boundary = self.staged_completion_boundary(&resolved_tab);
        let declared_mode = self
            .agent_registry
            .with_provider(&provider, |p| p.model_switch())
            .ok_or_else(|| {
                ModelSwitchError::NotSwitchable(format!("unknown agent provider: {provider}"))
            })?;
        if declared_mode == ModelSwitchMode::Unsupported {
            return Err(ModelSwitchError::Unsupported { provider });
        }
        crate::service::spawn::validate_agent_model(
            &self.agent_registry,
            &provider,
            Some(requested),
        )
        .map_err(ModelSwitchError::InvalidModel)?;
        if declared_mode == ModelSwitchMode::NextTurn
            && crate::service::codex_rpc::lifecycle::for_tab(&resolved_tab).is_some()
        {
            return crate::service::codex_rpc::model::switch_next_turn(
                self,
                &resolved_tab,
                &provider,
                requested,
                requested_effort,
            );
        }
        let declared_mode = if declared_mode == ModelSwitchMode::LiveCommand && restart_fallback {
            ModelSwitchMode::RestartResume
        } else {
            declared_mode
        };
        let snap = self.snapshot_tab_for_restart(&resolved_tab)?;
        if let Some(request) = staged {
            request.require_mechanism(declared_mode)?;
        }
        let session_id = self.verified_resumable_session(&snap)?;
        let effort = requested_effort.map(str::to_owned).or_else(|| {
            let saved = self
                .mux
                .read()
                .get_tab(&snap.tab_id)
                .and_then(|tab| recorded_launch_parameters(tab).resolved_reasoning_effort);
            saved.filter(|effort| {
                crate::service::spawn::validate_agent_reasoning_effort(
                    &self.agent_registry,
                    &provider,
                    Some(requested),
                    Some(effort),
                )
                .is_ok()
            })
        });
        crate::service::spawn::validate_agent_reasoning_effort(
            &self.agent_registry,
            &provider,
            Some(requested),
            effort.as_deref(),
        )
        .map_err(ModelSwitchError::NotSwitchable)?;
        let previous_model = {
            let mux = self.mux.read();
            mux.get_tab(&snap.tab_id).and_then(|tab| {
                tab.agent_session_model
                    .clone()
                    .or_else(|| tab.agent_provider_resolved_model.clone())
            })
        };
        if let Some(request) = staged {
            request.require_snapshot(&snap)?;
            request.require_current(self)?;
        } else if !self.tab_is_idle(&snap.tab_id) {
            return self.stage_model_switch(
                &snap,
                &session_id,
                requested,
                effort.as_deref(),
                staging::StagingParams {
                    mechanism: declared_mode,
                    force,
                    restart_fallback,
                    previous_model,
                    completion_boundary: completion_boundary.ok_or_else(|| {
                        ModelSwitchError::NotSwitchable(
                            "no verified own-turn completion boundary for staging".into(),
                        )
                    })?,
                },
            );
        } else {
            self.cancel_staged_model_switch(&snap.tab_id);
        }
        if declared_mode == ModelSwitchMode::LiveCommand {
            return self.switch_model_by_command(
                &snap,
                &session_id,
                requested,
                effort.as_deref(),
                staged.is_some(),
            );
        }
        if declared_mode == ModelSwitchMode::NextTurn {
            return self.switch_model_on_next_turn(
                &snap,
                &session_id,
                requested,
                effort.as_deref(),
                previous_model,
            );
        }
        let live_result = if declared_mode == ModelSwitchMode::Live {
            Some(
                crate::service::structured_prompt::live_switch_for_tab(
                    &snap.tab_id,
                    &session_id,
                    requested,
                    effort.as_deref(),
                )
                .map_err(ModelSwitchError::LiveSwitchFailed)?,
            )
        } else {
            None
        };
        let (mechanism, fallback_reason) =
            effective_model_switch_mode(declared_mode, live_result.as_ref());
        if staged.is_some() && fallback_reason.is_some() {
            return Err(ModelSwitchError::LiveSwitchFailed(
                fallback_reason.unwrap_or_default(),
            ));
        }
        if let Some(crate::service::structured_prompt::LiveModelSwitchResult::Applied {
            observed_model,
            confirmation_detail,
        }) = live_result
        {
            self.record_live_model_request(
                &snap.tab_id,
                &provider,
                &session_id,
                requested,
                effort.as_deref(),
            )?;
            let confirmed = observed_model.as_deref().is_some_and(|observed| {
                crate::service::structured_prompt::same_model_id(requested, observed)
                    && self.record_live_model_confirmation(
                        &snap.tab_id,
                        &provider,
                        &session_id,
                        requested,
                        observed,
                    )
            });
            let detail = if confirmed {
                None
            } else {
                confirmation_detail.or_else(|| {
                    Some("the bound session model has not confirmed the request".into())
                })
            };
            let confirmation = if confirmed {
                ModelSwitchConfirmation::Observed {
                    model: observed_model.clone().unwrap_or_default(),
                    evidence: ModelSwitchEvidence::LiveSessionRecord,
                }
            } else {
                ModelSwitchConfirmation::Pending
            };
            return Ok(ModelSwitchOutcome {
                tab_id: snap.tab_id,
                provider,
                session_id,
                restart: None,
                mechanism: ModelSwitchMode::Live,
                fallback_reason: None,
                previous_model,
                requested_model: requested.to_string(),
                launched_model: requested.to_string(),
                confirmation_detail: detail,
                confirmation,
            });
        }
        if mechanism == ModelSwitchMode::Unsupported {
            return Err(ModelSwitchError::Unsupported { provider });
        }
        self.require_background_work_safe(&snap.tab_id, force)?;
        if !self.tab_is_idle(&snap.tab_id) {
            let suffix = fallback_reason
                .as_deref()
                .map(|reason| format!(" Live mode was unavailable: {reason}."))
                .unwrap_or_default();
            return Err(ModelSwitchError::NotSwitchable(format!(
                "the agent is mid-turn or waiting on a prompt; RestartResume requires an idle tab.{suffix}"
            )));
        }
        let mut launch = ModelSwitchLaunch::prepare(self, &snap, &session_id, requested)?;
        launch.params.reasoning_effort = effort.clone();
        launch.launch.requested_reasoning_effort = effort.clone();
        launch.launch.resolved_reasoning_effort = effort;
        let launched_model = launch.launched_model().to_string();
        log::info!(
            "[agent-model] tab={} provider={} session={} {:?} -> {}",
            snap.tab_id,
            provider,
            session_id,
            previous_model,
            launched_model
        );
        let restart = self.restart_live_agent(
            snap,
            force,
            DEFAULT_GRACE_SECS,
            Some(&launch),
            super::RestartMode::Standard,
        )?;
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
            tab_id: restart.tab_id.clone(),
            provider: restart.provider.clone(),
            session_id: session_id.clone(),
            restart: Some(restart),
            mechanism,
            fallback_reason,
            previous_model,
            requested_model: requested.to_string(),
            launched_model,
            confirmation_detail: None,
            confirmation,
        })
    }

    pub(crate) fn record_live_model_request(
        &self,
        tab_id: &str,
        provider: &str,
        session_id: &str,
        requested_model: &str,
        effort: Option<&str>,
    ) -> Result<(), ModelSwitchError> {
        self.record_model_request(tab_id, provider, session_id, requested_model, effort, None)
    }

    pub(crate) fn record_model_request(
        &self,
        tab_id: &str,
        provider: &str,
        session_id: &str,
        requested_model: &str,
        effort: Option<&str>,
        boundary: Option<codeterm_types::agent::SessionModelTranscriptBoundary>,
    ) -> Result<(), ModelSwitchError> {
        let updated = {
            let mut mux = self.mux.write();
            let Some(tab) = mux.get_tab_mut(tab_id) else {
                return Err(ModelSwitchError::NotFound(format!(
                    "Tab {tab_id} not found"
                )));
            };
            if tab.agent_type.as_deref() != Some(provider)
                || tab.agent_session_id.as_deref() != Some(session_id)
                || tab.session_binding_nonce.is_none()
                || tab.session_binding_nonce != tab.reported_session_binding_nonce
            {
                return Err(ModelSwitchError::NotSwitchable(
                    "the tab's verified session binding changed during the live model switch"
                        .into(),
                ));
            }
            record_live_request_identity(tab, requested_model, effort);
            if let Some(boundary) = boundary {
                next_turn::record_pending_turn(tab, session_id, requested_model, boundary);
            }
            true
        };
        if updated {
            if let Err(error) = crate::service::session_persist::persist_session_after_mutation(
                self,
                crate::service::session_persist::SessionPersistUrgency::Prompt,
            ) {
                log::warn!(
                    "[agent-model] tab={tab_id} failed to persist the live model request: {error}"
                );
            }
            self.persist_current_attempt_session_model(tab_id);
            self.emit_state_changed();
        }
        Ok(())
    }

    fn record_live_model_confirmation(
        &self,
        tab_id: &str,
        provider: &str,
        session_id: &str,
        requested_model: &str,
        observed_model: &str,
    ) -> bool {
        if !crate::service::session_model::session_model_matches(
            &self.agent_registry,
            provider,
            requested_model,
            observed_model,
        ) {
            return false;
        }
        let binding_nonce = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(tab_id) else {
                return false;
            };
            if tab.agent_type.as_deref() != Some(provider)
                || tab.agent_session_id.as_deref() != Some(session_id)
                || tab.session_binding_nonce.is_none()
                || tab.session_binding_nonce != tab.reported_session_binding_nonce
            {
                return false;
            }
            tab.session_binding_nonce.clone().unwrap_or_default()
        };
        self.apply_session_model(
            tab_id,
            provider,
            session_id,
            observed_model.to_string(),
            None,
            Some(&binding_nonce),
            false,
        );
        let mux = self.mux.read();
        mux.get_tab(tab_id).is_some_and(|tab| {
            live_model_confirmation_matches(
                &self.agent_registry,
                tab,
                provider,
                session_id,
                &binding_nonce,
                observed_model,
            )
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
                let identity_current =
                    crate::service::session_model::has_current_session_model(tab, session_id);
                (
                    crate::service::session_model::current_launch_report(tab, session_id),
                    identity_current
                        .then(|| tab.agent_session_model.clone())
                        .flatten(),
                    tab.pending_model_switch_observation
                        .as_ref()
                        .filter(|pending| pending.session_id == session_id)
                        .map(|pending| pending.launched_model.clone()),
                )
            };
            if pending_model.is_none() {
                if let Some(report) = report {
                    if crate::service::session_model::session_model_matches(
                        &self.agent_registry,
                        provider,
                        launched_model,
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
