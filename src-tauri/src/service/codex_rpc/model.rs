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
    Ok(())
}

pub fn request_mode(tab: &str, mode: RuntimeMode) -> Result<(), String> {
    let sidecar = for_tab(tab).ok_or("No structured control endpoint")?;
    sidecar.controls.lock().mode = mode;
    Ok(())
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
        svc.apply_session_model(
            tab,
            &provider,
            thread,
            model.into(),
            None,
            Some(&nonce),
            false,
        );
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
    svc.record_live_model_request(tab_id, provider, &session_id, requested, effort.as_deref())?;
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

impl crate::service::ServiceContext {
    pub(crate) fn agent_switch_model_with_controls(
        &self,
        tab: &str,
        model: &str,
        effort: Option<&str>,
        mode: Option<RuntimeMode>,
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
        let outcome = self.agent_switch_model_with_reasoning(tab, model, effort)?;
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
mod tests {
    use super::*;
    #[test]
    fn codex_rpc_control_runtime_modes_and_next_turn_overrides() {
        for (mode, policy, reviewer, sandbox) in [
            (
                RuntimeMode::ApprovalRequired,
                "untrusted",
                "user",
                "readOnly",
            ),
            (
                RuntimeMode::AutoAcceptEdits,
                "on-request",
                "user",
                "workspaceWrite",
            ),
            (
                RuntimeMode::Auto,
                "on-request",
                "auto_review",
                "workspaceWrite",
            ),
            (RuntimeMode::FullAccess, "never", "user", "dangerFullAccess"),
        ] {
            let controls = TurnControls {
                model: Some("selected".into()),
                effort: Some("high".into()),
                mode,
            };
            let value = controls.overrides();
            assert_eq!(value["model"], "selected");
            assert_eq!(value["effort"], "high");
            assert_eq!(value["approvalPolicy"], policy);
            assert_eq!(value["approvalsReviewer"], reviewer);
            assert_eq!(value["sandboxPolicy"]["type"], sandbox);
        }
    }
}
