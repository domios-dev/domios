use super::lifecycle::for_tab;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeMode {
    ApprovalRequired,
    #[default]
    AutoAcceptEdits,
    Auto,
    FullAccess,
}

impl RuntimeMode {
    pub fn overrides(self) -> Value {
        let (approval, reviewer, sandbox) = match self {
            Self::ApprovalRequired => ("untrusted", "user", "readOnly"),
            Self::AutoAcceptEdits => ("on-request", "user", "workspaceWrite"),
            Self::Auto => ("on-request", "auto_review", "workspaceWrite"),
            Self::FullAccess => ("never", "user", "dangerFullAccess"),
        };
        json!({"approvalPolicy":approval,"approvalsReviewer":reviewer,"sandboxPolicy":{"type":sandbox}})
    }
}

#[derive(Default)]
pub struct TurnControls {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub mode: RuntimeMode,
    pub pending_settings: Option<Value>,
}

impl TurnControls {
    pub fn overrides(&self) -> Value {
        let mut value = self.mode.overrides();
        if let Some(model) = &self.model {
            value["model"] = json!(model);
        }
        if let Some(effort) = &self.effort {
            value["effort"] = json!(effort);
        }
        value
    }
}

pub fn turn_overrides(tab: &str) -> Value {
    for_tab(tab)
        .map(|sidecar| sidecar.controls.lock().overrides())
        .unwrap_or_else(|| json!({}))
}

pub fn request_model(tab: &str, model: &str, effort: Option<&str>) -> Result<(), String> {
    let sidecar = for_tab(tab).ok_or("No structured control endpoint")?;
    let mut controls = sidecar.controls.lock();
    controls.model = Some(model.into());
    controls.effort = effort.map(str::to_owned);
    controls.pending_settings = None;
    Ok(())
}

pub fn request_mode(tab: &str, mode: RuntimeMode) -> Result<(), String> {
    let sidecar = for_tab(tab).ok_or("No structured control endpoint")?;
    sidecar.controls.lock().mode = mode;
    Ok(())
}

pub(super) fn session_effort(settings: &Value) -> Option<Option<String>> {
    match settings.get("effort")? {
        Value::Null => Some(None),
        Value::String(effort) if !effort.trim().is_empty() => Some(Some(effort.trim().into())),
        _ => None,
    }
}

fn new_turn_settings(
    boundary: &codeterm_types::agent::SessionModelTranscriptBoundary,
    turn: Option<&str>,
    requested_effort: Option<&str>,
    settings: &Value,
) -> bool {
    turn.is_some_and(|turn| boundary.last_entry_uuid.as_deref() != Some(turn))
        && requested_effort
            .is_none_or(|effort| session_effort(settings).flatten().as_deref() == Some(effort))
}

fn cache_requested_settings(model_matches: bool, effort: Option<&str>, settings: &Value) -> bool {
    model_matches
        && effort.is_none_or(|effort| session_effort(settings).flatten().as_deref() == Some(effort))
}

pub(super) fn on_turn_started(svc: &crate::service::ServiceContext, tab: &str, thread: &str) {
    let settings = for_tab(tab).and_then(|sidecar| sidecar.controls.lock().pending_settings.take());
    if let Some(settings) = settings {
        observe(svc, tab, thread, &settings);
    }
}

pub fn observe(svc: &crate::service::ServiceContext, tab: &str, thread: &str, params: &Value) {
    let settings = params.get("threadSettings").unwrap_or(params);
    let Some(model) = settings.get("model").and_then(Value::as_str) else {
        return;
    };
    let identity = svc.mux.read().get_tab(tab).and_then(|tab| {
        (tab.agent_session_id.as_deref() == Some(thread))
            .then(|| (tab.agent_type.clone(), tab.session_binding_nonce.clone()))
    });
    if let Some((Some(provider), Some(nonce))) = identity {
        let pending = svc.mux.read().get_tab(tab).and_then(|tab| {
            tab.pending_model_switch_observation
                .clone()
                .map(|pending| (pending, tab.reasoning_effort_identity.requested.clone()))
        });
        if let Some((pending, effort)) = pending.as_ref() {
            let active = super::control::active_turn(tab);
            if pending.session_id != thread
                || !new_turn_settings(
                    &pending.transcript_boundary,
                    active.as_deref(),
                    effort.as_deref(),
                    settings,
                )
            {
                if let Some(sidecar) = for_tab(tab).filter(|_| {
                    cache_requested_settings(
                        crate::service::session_model::session_model_matches(
                            &svc.agent_registry,
                            &provider,
                            &pending.launched_model,
                            model,
                        ),
                        effort.as_deref(),
                        settings,
                    )
                }) {
                    sidecar.controls.lock().pending_settings = Some(params.clone());
                }
                return;
            }
        }
        svc.apply_session_model(
            tab,
            &provider,
            thread,
            model.into(),
            None,
            Some(&nonce),
            pending.is_some(),
        );
        if let Some(actual) = session_effort(settings) {
            svc.record_session_reasoning_effort(tab, &provider, thread, Some(&nonce), actual);
        }
        if let Some((pending, _)) = pending {
            if svc.clear_pending_model_switch(tab, &pending, &nonce) {
                svc.emit_state_changed();
            }
        }
    }
}

pub(crate) fn switch_next_turn(
    svc: &crate::service::ServiceContext,
    tab_id: &str,
    provider: &str,
    requested: &str,
    requested_effort: Option<&str>,
) -> Result<
    crate::service::agent_restart::ModelSwitchOutcome,
    crate::service::agent_restart::ModelSwitchError,
> {
    use crate::service::agent_restart::{
        ModelSwitchConfirmation, ModelSwitchError, ModelSwitchOutcome,
    };
    let (session_id, previous_model, effort) = {
        let mux = svc.mux.read();
        let tab = mux
            .get_tab(tab_id)
            .ok_or_else(|| ModelSwitchError::NotFound(tab_id.into()))?;
        (
            tab.agent_session_id
                .clone()
                .ok_or_else(|| ModelSwitchError::NotSwitchable("No bound thread".into()))?,
            tab.agent_session_model.clone(),
            requested_effort
                .map(str::to_owned)
                .or_else(|| tab.restorable_launch.resolved_reasoning_effort.clone()),
        )
    };
    crate::service::spawn::validate_agent_reasoning_effort(
        &svc.agent_registry,
        provider,
        Some(requested),
        effort.as_deref(),
    )
    .map_err(ModelSwitchError::NotSwitchable)?;
    let boundary = codeterm_types::agent::SessionModelTranscriptBoundary {
        last_entry_uuid: super::control::active_turn(tab_id),
        last_entry_type: Some("rpc_turn".into()),
        ..Default::default()
    };
    svc.record_model_request(
        tab_id,
        provider,
        &session_id,
        requested,
        effort.as_deref(),
        Some(boundary),
    )?;
    request_model(tab_id, requested, effort.as_deref())
        .map_err(ModelSwitchError::LiveSwitchFailed)?;
    Ok(ModelSwitchOutcome {
        tab_id: tab_id.into(),
        provider: provider.into(),
        session_id,
        restart: None,
        mechanism: crate::agent::provider::ModelSwitchMode::NextTurn,
        fallback_reason: None,
        previous_model,
        requested_model: requested.into(),
        launched_model: requested.into(),
        confirmation_detail: None,
        confirmation: ModelSwitchConfirmation::Pending,
    })
}

pub(crate) struct ModelSwitchOptions {
    pub force: bool,
    pub restart_resume: bool,
}

impl crate::service::ServiceContext {
    pub(crate) fn agent_switch_model_with_restart_fallback(
        &self,
        tab: &str,
        model: &str,
        effort: Option<&str>,
        mode: Option<RuntimeMode>,
        options: ModelSwitchOptions,
    ) -> Result<
        crate::service::agent_restart::ModelSwitchOutcome,
        crate::service::agent_restart::ModelSwitchError,
    > {
        if !options.restart_resume {
            return self.agent_switch_model_with_controls_and_force(
                tab,
                model,
                effort,
                mode,
                options.force,
            );
        }
        if mode.is_some() {
            return Err(
                crate::service::agent_restart::ModelSwitchError::NotSwitchable(
                    "restart fallback cannot change runtime mode".into(),
                ),
            );
        }
        self.agent_switch_model_with_fallback(tab, model, effort, options.force, true)
    }

    pub(crate) fn agent_switch_model_with_controls_and_force(
        &self,
        tab: &str,
        model: &str,
        effort: Option<&str>,
        mode: Option<RuntimeMode>,
        force: bool,
    ) -> Result<
        crate::service::agent_restart::ModelSwitchOutcome,
        crate::service::agent_restart::ModelSwitchError,
    > {
        use crate::service::agent_restart::ModelSwitchError;
        if mode.is_some() && for_tab(tab).is_none() {
            return Err(ModelSwitchError::NotSwitchable(
                "Runtime mode requires a structured control endpoint".into(),
            ));
        }
        let outcome = self.agent_switch_model_with_options(tab, model, effort, force)?;
        if let Some(mode) = mode {
            request_mode(tab, mode).map_err(ModelSwitchError::LiveSwitchFailed)?;
            if let Some(tab) = self.mux.write().get_tab_mut(tab) {
                tab.structured_runtime_mode = serde_json::to_value(mode)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned));
                tab.restorable_launch.structured_runtime_mode = tab.structured_runtime_mode.clone();
            }
            crate::service::session_persist::persist_session_after_mutation(
                self,
                crate::service::session_persist::SessionPersistUrgency::Prompt,
            )
            .map_err(ModelSwitchError::LiveSwitchFailed)?;
            self.emit_state_changed();
        }
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests;
