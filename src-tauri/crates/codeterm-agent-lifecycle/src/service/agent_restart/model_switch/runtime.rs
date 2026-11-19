use super::*;
use crate::service::agent_restart::runtime::RuntimeRestartExt;
use crate::service::agent_runtime::{engine, restart::ModelSelection};

pub(super) fn switch(
    svc: &ServiceContext,
    tab: &str,
    requested: &str,
    effort: Option<&str>,
    force: bool,
) -> Result<ModelSwitchOutcome, ModelSwitchError> {
    let (provider, binding_nonce, thread_id, previous_model, current_model) = {
        let mux = svc.mux.read();
        let target = mux
            .get_tab(tab)
            .ok_or_else(|| ModelSwitchError::NotFound(tab.into()))?;
        (
            target
                .agent_type
                .clone()
                .ok_or_else(|| ModelSwitchError::NotFound(tab.into()))?,
            target
                .session_binding_nonce
                .clone()
                .ok_or_else(|| ModelSwitchError::NotSwitchable("Runtime binding absent".into()))?,
            target.agent_session_id.clone(),
            target.agent_session_model.clone(),
            target
                .agent_runtime_config
                .as_ref()
                .and_then(|config| config["model"].as_str())
                .map(str::to_owned),
        )
    };
    let selected = crate::service::spawn::resolve_agent_model_selection(
        &svc.agent_registry,
        Some(&provider),
        Some(requested),
        &provider,
    )
    .map_err(ModelSwitchError::InvalidModel)?;
    let model = selected
        .model
        .or(current_model)
        .ok_or_else(|| ModelSwitchError::NotSwitchable("Runtime model selection absent".into()))?;
    let effort = effort.map(str::trim).filter(|value| !value.is_empty());
    if let Some(effort) = effort {
        let supported = svc
            .agent_registry
            .with_provider(&provider, |provider| {
                let levels = provider.reasoning_efforts_for_model(Some(&model));
                let levels = if levels.is_empty() {
                    provider.reasoning_efforts()
                } else {
                    levels
                };
                levels.into_iter().map(|level| level.id).collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !supported.iter().any(|level| level == effort) {
            return Err(ModelSwitchError::UnsupportedReasoningEffort {
                model,
                requested: effort.into(),
                supported,
            });
        }
    }
    let selection = ModelSelection {
        provider: provider.clone(),
        binding_nonce,
        thread_id,
        model: model.clone(),
        effort: effort.map(str::to_owned),
    };
    let restart = svc.restart_runtime_agent_with_selection(
        tab,
        force,
        false,
        false,
        Some(crate::service::agent_runtime::restart::RestartSelection::Model(&selection)),
    )?;
    let settings = engine::for_tab(tab).map(|session| session.context.lock().settings.clone());
    let confirmation = if restart.agent_ready
        && settings
            .as_ref()
            .is_some_and(|settings| settings.model.as_deref() == Some(model.as_str()))
    {
        ModelSwitchConfirmation::Observed {
            model: model.clone(),
            evidence: ModelSwitchEvidence::SessionStartReport,
        }
    } else {
        ModelSwitchConfirmation::Pending
    };
    Ok(ModelSwitchOutcome {
        tab_id: tab.into(),
        provider,
        session_id: restart.session_id.clone().unwrap_or_default(),
        restart: Some(restart),
        mechanism: ModelSwitchMode::RestartResume,
        fallback_reason: None,
        previous_model,
        requested_model: model.clone(),
        launched_model: model,
        confirmation_detail: None,
        confirmation,
        control_changes: Vec::new(),
    })
}
