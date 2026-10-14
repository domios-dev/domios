use super::{engine, model::RuntimeFailure};
use crate::multiplexer::types::TabBacking;
use crate::service::agent_restart::{
    ModelSwitchConfirmation, ModelSwitchError, ModelSwitchEvidence, ModelSwitchOutcome,
};
use crate::service::{agent_runtime::model::RuntimeMode, ServiceContext};
use codeterm_plugin::agent_runtime::{RuntimeOperation, RuntimeResult, RuntimeSettings};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SwitchRoute {
    Runtime,
    Pty,
}

fn switch_route(runtime_capability: bool, backing: &TabBacking) -> SwitchRoute {
    if runtime_capability && matches!(backing, TabBacking::ChatBackend { .. }) {
        SwitchRoute::Runtime
    } else {
        SwitchRoute::Pty
    }
}

pub(crate) fn routed_tab(svc: &ServiceContext, tab: &str) -> Option<String> {
    let (tab, provider, backing) = {
        let mux = svc.mux.read();
        let id = mux.resolve_tab_id(tab)?;
        let target = mux.get_tab(&id)?;
        (id, target.agent_type.clone()?, target.backing.clone())
    };
    (switch_route(engine::requested(svc, &provider), &backing) == SwitchRoute::Runtime)
        .then_some(tab)
}

pub(crate) fn switch(
    svc: &ServiceContext,
    tab: &str,
    model: &str,
    effort: Option<&str>,
    mode: Option<RuntimeMode>,
) -> Result<ModelSwitchOutcome, ModelSwitchError> {
    if !model.trim().is_empty() {
        let provider = svc
            .mux
            .read()
            .get_tab(tab)
            .and_then(|target| target.agent_type.clone())
            .ok_or_else(|| ModelSwitchError::NotFound(tab.into()))?;
        crate::service::spawn::validate_agent_model(
            &svc.agent_registry,
            &provider,
            Some(model.trim()),
        )
        .map_err(ModelSwitchError::InvalidModel)?;
    }
    tauri::async_runtime::block_on(async {
        switch_inner(svc, tab, model, effort, mode)
            .await
            .map_err(|reason| ModelSwitchError::RuntimeControlRefused(reason.to_string()))
    })
}

async fn switch_inner(
    svc: &ServiceContext,
    tab: &str,
    model: &str,
    effort: Option<&str>,
    mode: Option<RuntimeMode>,
) -> Result<ModelSwitchOutcome, RuntimeFailure> {
    let session = engine::ensure(svc, tab).await?;
    let _permit = session
        .gate
        .try_acquire()
        .map_err(|_| RuntimeFailure::Refused("Runtime operation already in progress".into()))?;
    engine::validate_binding(svc, &session)?;
    let before = session.context.lock().clone();
    let thread = before.thread_id.clone().ok_or(RuntimeFailure::Superseded)?;
    let mut settings = before.settings.clone();
    if !model.trim().is_empty() {
        settings.model = Some(model.trim().into());
    }
    if let Some(effort) = effort {
        settings.effort = Some(effort.into());
    }
    if let Some(mode) = mode {
        settings.mode = serde_json::to_value(mode)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned));
    }
    let operation = RuntimeOperation::Control {
        thread_id: Some(thread.clone()),
        action: "settings".into(),
        value: serde_json::to_value(&settings)
            .map_err(|reason| RuntimeFailure::Refused(reason.to_string()))?,
    };
    session.controls_confirmation.lock().take();
    let process = session
        .process
        .lock()
        .clone()
        .ok_or(RuntimeFailure::PipeEof)?;
    let result = engine::run(svc, &session.provider, &session, &process, operation).await?;
    engine::validate_binding(svc, &session)?;
    if session.context.lock().epoch != before.epoch {
        return Err(RuntimeFailure::Superseded);
    }
    let confirmed = session.controls_confirmation.lock().take();
    let confirmed = confirmed
        .filter(|confirmed| matches!(result, RuntimeResult::Done) && confirms(&settings, confirmed))
        .ok_or_else(|| {
            RuntimeFailure::Refused("Runtime did not confirm requested controls".into())
        })?;
    let actual_model = confirmed
        .model
        .clone()
        .ok_or_else(|| RuntimeFailure::Refused("Runtime model confirmation absent".into()))?;
    {
        let mut mux = svc.mux.write();
        let target = mux.get_tab_mut(tab).ok_or(RuntimeFailure::Superseded)?;
        if target.agent_type.as_deref() != Some(session.provider.as_str())
            || target.session_binding_nonce.as_deref() != Some(before.binding_nonce.as_str())
            || target.agent_session_id.as_deref() != Some(thread.as_str())
        {
            return Err(RuntimeFailure::Superseded);
        }
        target.restorable_launch.model = Some(actual_model.clone());
        if !model.trim().is_empty() {
            target.restorable_launch.requested_model = Some(model.trim().into());
            target.agent_requested_model = Some(model.trim().into());
        }
        target.restorable_launch.resolved_reasoning_effort = confirmed.effort.clone();
        target.restorable_launch.structured_runtime_mode = confirmed.mode.clone();
        if let (Some(options), Some(mode)) = (
            target.restorable_launch.runtime_launch_options.as_mut(),
            confirmed.mode.as_ref(),
        ) {
            options.mode = mode.clone();
        }
        target.pending_model_selection = None;
        target.pending_model_switch_observation = None;
        let config = target
            .agent_runtime_config
            .get_or_insert_with(|| serde_json::json!({}));
        for (key, value) in [
            ("model", &confirmed.model),
            ("effort", &confirmed.effort),
            ("mode", &confirmed.mode),
        ] {
            if let Some(value) = value {
                config[key] = value.clone().into();
            }
        }
    }
    crate::service::session_persist::persist_session_after_mutation(
        svc,
        crate::service::session_persist::SessionPersistUrgency::Prompt,
    )
    .map_err(RuntimeFailure::Refused)?;
    svc.emit_state_changed();
    Ok(ModelSwitchOutcome {
        tab_id: tab.into(),
        provider: session.provider.clone(),
        session_id: thread,
        restart: None,
        mechanism: crate::agent::provider::ModelSwitchMode::Live,
        fallback_reason: None,
        previous_model: before.settings.model,
        requested_model: model.into(),
        launched_model: actual_model.clone(),
        confirmation_detail: None,
        confirmation: ModelSwitchConfirmation::Observed {
            model: actual_model,
            evidence: ModelSwitchEvidence::LiveSessionRecord,
        },
    })
}

fn confirms(requested: &RuntimeSettings, confirmed: &RuntimeSettings) -> bool {
    [
        (&requested.model, &confirmed.model),
        (&requested.effort, &confirmed.effort),
        (&requested.mode, &confirmed.mode),
    ]
    .into_iter()
    .all(|(requested, confirmed)| requested.is_none() || requested == confirmed)
}

#[cfg(test)]
mod tests;

mod interrupt;

pub fn interrupt(svc: &ServiceContext, tab: &str) -> Result<(), String> {
    interrupt::interrupt(svc, tab)
}

pub(super) fn observe_event(session: &engine::Session, event: &super::events::RuntimeEvent) {
    if let super::events::RuntimeEvent::TurnUpdated {
        thread_id,
        turn_id: Some(turn),
        state: codeterm_plugin::agent_runtime::RuntimeTurnState::Idle,
        ..
    } = event
    {
        engine::record_completed_turn(session, thread_id, Some(turn));
    }
}
