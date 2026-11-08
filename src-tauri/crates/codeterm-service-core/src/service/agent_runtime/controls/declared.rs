use super::*;
use crate::service::agent_runtime::{declared_controls, profile_options, selections};
use std::sync::Arc;

pub(super) fn switch(
    svc: &ServiceContext,
    tab: &str,
    session: &Arc<engine::Session>,
    model: &str,
    effort: Option<&str>,
    mode: Option<&str>,
    service_tier: Option<codeterm_plugin::agent_runtime::RuntimeServiceTierSelection>,
) -> Result<ModelSwitchOutcome, ModelSwitchError> {
    let refused = |reason: String| ModelSwitchError::RuntimeControlRefused(reason);
    if service_tier.is_some() {
        return Err(refused(
            "Use the session's declared controls for service tier changes".into(),
        ));
    }
    let before = session.context.lock().clone();
    let mode = mode.map(str::trim).filter(|value| !value.is_empty());
    if let Some(requested) = mode {
        let supported = before
            .settings
            .modes
            .iter()
            .flatten()
            .map(|mode| mode.id.clone())
            .collect::<Vec<_>>();
        if !supported.iter().any(|id| id == requested) {
            return Err(ModelSwitchError::UnsupportedRuntimeMode {
                requested: requested.to_owned(),
                supported,
            });
        }
    }
    let options = before
        .settings
        .config_options
        .as_deref()
        .unwrap_or_default();
    let model = model.trim();
    let model = if model.is_empty() {
        None
    } else {
        Some(profile_options::resolve(options, "model", model).map_err(refused)?)
    };
    let effort = effort
        .map(|value| profile_options::resolve(options, "thought_level", value))
        .transpose()
        .map_err(refused)?;
    super::super::capacity_retry::cancel(svc, tab).map_err(refused)?;
    crate::async_runtime::block_on(async {
        for (category, value) in [("model", &model), ("thought_level", &effort)] {
            let Some(value) = value else { continue };
            let current = session.context.lock().clone();
            if current.epoch != before.epoch
                || current.thread_id != before.thread_id
                || current.binding_nonce != before.binding_nonce
            {
                return Err(refused("Runtime session changed during selection".into()));
            }
            let option = current
                .settings
                .config_options
                .as_ref()
                .and_then(|options| {
                    options
                        .iter()
                        .find(|option| option.category.as_deref() == Some(category))
                })
                .ok_or_else(|| refused("Runtime control is no longer declared".into()))?;
            if option.current_value == *value {
                continue;
            }
            declared_controls::select(
                svc,
                tab,
                declared_controls::Selection {
                    thread_id: current
                        .thread_id
                        .clone()
                        .ok_or_else(|| refused("Runtime thread unavailable".into()))?,
                    binding_nonce: current.binding_nonce,
                    epoch: current.epoch,
                    id: option.id.clone(),
                    value: value.clone(),
                    mode: false,
                },
            )
            .await
            .map_err(|error| match error {
                declared_controls::ControlError::Busy => ModelSwitchError::RuntimeBusy,
                declared_controls::ControlError::Refused(reason) => refused(reason),
            })?;
        }
        if let Some(requested) = mode {
            select_mode(svc, tab, session, &before, requested).await?;
        }
        let after = session.context.lock().settings.clone();
        let requested = RuntimeSettings {
            model: model.clone(),
            effort: effort.clone(),
            ..Default::default()
        };
        selections::persist(svc, session, &before, &requested, &after)
            .map_err(|reason| refused(reason.to_string()))?;
        // A session may report no model at all; only a requested model needs its confirmation.
        let actual = match after.model.clone() {
            Some(actual) => actual,
            None if model.is_none() && effort.is_none() => String::new(),
            None => return Err(refused("Runtime model confirmation absent".into())),
        };
        let control_changes = super::control_changes(&before.settings, &after);
        Ok(ModelSwitchOutcome {
            tab_id: tab.into(),
            provider: session.provider.clone(),
            session_id: before
                .thread_id
                .clone()
                .ok_or_else(|| refused("Runtime thread unavailable".into()))?,
            restart: None,
            mechanism: crate::agent::provider::ModelSwitchMode::Live,
            fallback_reason: None,
            control_changes,
            previous_model: before.settings.model,
            requested_model: model.unwrap_or_else(|| actual.clone()),
            launched_model: actual.clone(),
            confirmation_detail: None,
            confirmation: ModelSwitchConfirmation::Observed {
                model: actual,
                evidence: ModelSwitchEvidence::LiveSessionRecord,
            },
        })
    })
}

/// The same selection the picker's mode row makes; the route admits owner callers only.
async fn select_mode(
    svc: &ServiceContext,
    tab: &str,
    session: &Arc<engine::Session>,
    before: &codeterm_plugin::agent_runtime::RuntimeContext,
    requested: &str,
) -> Result<(), ModelSwitchError> {
    let refused = |reason: &str| ModelSwitchError::RuntimeControlRefused(reason.into());
    let current = session.context.lock().clone();
    if current.epoch != before.epoch
        || current.thread_id != before.thread_id
        || current.binding_nonce != before.binding_nonce
    {
        return Err(refused("Runtime session changed during selection"));
    }
    if current.settings.effective_mode() == Some(requested) {
        return Ok(());
    }
    declared_controls::select(
        svc,
        tab,
        declared_controls::Selection {
            thread_id: current
                .thread_id
                .clone()
                .ok_or_else(|| refused("Runtime thread unavailable"))?,
            binding_nonce: current.binding_nonce,
            epoch: current.epoch,
            id: String::new(),
            value: requested.to_owned(),
            mode: true,
        },
    )
    .await
    .map_err(|error| match error {
        declared_controls::ControlError::Busy => ModelSwitchError::RuntimeBusy,
        declared_controls::ControlError::Refused(reason) => {
            ModelSwitchError::RuntimeControlRefused(reason)
        }
    })?;
    if session.context.lock().settings.effective_mode() != Some(requested) {
        return Err(refused("Runtime did not confirm the requested mode"));
    }
    Ok(())
}
