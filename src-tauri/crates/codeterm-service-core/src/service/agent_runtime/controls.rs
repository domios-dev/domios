use super::{engine, model::RuntimeFailure};
use crate::multiplexer::types::TabBacking;
use crate::service::agent_restart::{
    ModelSwitchConfirmation, ModelSwitchError, ModelSwitchEvidence, ModelSwitchOutcome,
};
use crate::service::MuxEmitCause;
use crate::service::{agent_runtime::model::RuntimeMode, ServiceContext};
use codeterm_plugin::agent_runtime::{RuntimeOperation, RuntimeResult, RuntimeSettings};

mod declared;

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

impl ServiceContext {
    pub fn agent_switch_model_with_restart_fallback(
        &self,
        tab: &str,
        model: &str,
        effort: Option<&str>,
        mode: Option<&str>,
        options: super::model::ModelSwitchOptions,
    ) -> Result<ModelSwitchOutcome, ModelSwitchError> {
        if let Some(tab) = routed_tab(self, tab) {
            return switch(self, &tab, model, effort, mode, options.service_tier);
        }
        if options.service_tier.is_some() {
            return Err(ModelSwitchError::NotSwitchable(
                "Fast mode requires a capable runtime".into(),
            ));
        }
        if mode.is_some() {
            return Err(ModelSwitchError::NotSwitchable(
                if options.restart_resume {
                    "restart fallback cannot change runtime mode"
                } else {
                    "Runtime mode requires a capable runtime"
                }
                .into(),
            ));
        }
        self.agent_switch_model_with_fallback(
            tab,
            model,
            effort,
            options.force,
            options.restart_resume,
        )
    }
}

pub(crate) fn switch(
    svc: &ServiceContext,
    tab: &str,
    model: &str,
    effort: Option<&str>,
    mode: Option<&str>,
    service_tier: Option<codeterm_plugin::agent_runtime::RuntimeServiceTierSelection>,
) -> Result<ModelSwitchOutcome, ModelSwitchError> {
    let provider = svc
        .mux
        .read()
        .get_tab(tab)
        .and_then(|target| target.agent_type.clone())
        .ok_or_else(|| ModelSwitchError::NotFound(tab.into()))?;
    // A switch addresses the tab the same way a send does, so it wakes a sleeping tab.
    if svc
        .mux
        .read()
        .get_tab(tab)
        .is_some_and(|target| target.frozen)
    {
        svc.wake_frozen_tab(tab)
            .map_err(ModelSwitchError::RuntimeControlRefused)?;
    }
    let live = engine::for_tab(tab);
    // `ensure` waits on the gate with no busy outcome, so a running operation is waited out here first.
    if live.as_ref().is_some_and(|session| {
        crate::async_runtime::block_on(async {
            tokio::time::timeout(GATE_WAIT, session.gate.acquire())
                .await
                .is_err()
        })
    }) {
        return Err(ModelSwitchError::RuntimeBusy);
    }
    let declared_controls = live
        .as_ref()
        .is_some_and(|session| session.context.lock().settings.config_options.is_some())
        || svc.mux.read().get_tab(tab).is_some_and(|target| {
            target
                .agent_runtime_state
                .as_ref()
                .and_then(|state| state.pointer("/controls/configOptions"))
                .is_some_and(serde_json::Value::is_array)
        });
    if declared_controls {
        let session = crate::async_runtime::block_on(engine::ensure(svc, tab))
            .map_err(|reason| ModelSwitchError::RuntimeControlRefused(reason.to_string()))?;
        return declared::switch(svc, tab, &session, model, effort, mode, service_tier);
    }
    let model_selection = crate::service::spawn::resolve_agent_model_selection(
        &svc.agent_registry,
        Some(&provider),
        Some(model),
        &provider,
    )
    .map_err(ModelSwitchError::InvalidModel)?;
    let model = model_selection.model.as_deref().unwrap_or_default();
    super::capacity_retry::cancel(svc, tab).map_err(ModelSwitchError::RuntimeControlRefused)?;
    let effective_model = svc
        .mux
        .read()
        .get_tab(tab)
        .and_then(|target| {
            model_selection
                .model
                .clone()
                .or_else(|| {
                    target
                        .agent_runtime_config
                        .as_ref()
                        .and_then(|config| config["model"].as_str())
                        .map(str::to_owned)
                })
                .or_else(|| target.agent_session_model.clone())
                .or_else(|| target.agent_provider_resolved_model.clone())
        })
        .unwrap_or_default();
    if let Some(requested) = effort.map(str::trim).filter(|value| !value.is_empty()) {
        let supported = svc
            .agent_registry
            .with_provider(&provider, |provider| {
                let model_levels = provider.reasoning_efforts_for_model(Some(&effective_model));
                let levels = if model_levels.is_empty() {
                    provider.reasoning_efforts()
                } else {
                    model_levels
                };
                levels.into_iter().map(|level| level.id).collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !supported.iter().any(|level| level == requested) {
            return Err(ModelSwitchError::UnsupportedReasoningEffort {
                model: effective_model,
                requested: requested.to_owned(),
                supported,
            });
        }
    }
    let mode = if let Some(requested) = mode.map(str::trim).filter(|value| !value.is_empty()) {
        let supported = svc
            .agent_registry
            .with_provider(&provider, |provider| {
                provider
                    .agent_runtime()
                    .map(|control| {
                        control
                            .runtime_modes
                            .iter()
                            .map(|mode| mode.id.clone())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        if !supported.iter().any(|value| value == requested) {
            return Err(ModelSwitchError::UnsupportedRuntimeMode {
                requested: requested.to_owned(),
                supported,
            });
        }
        Some(
            serde_json::from_value(serde_json::json!(requested)).map_err(|_| {
                ModelSwitchError::UnsupportedRuntimeMode {
                    requested: requested.to_owned(),
                    supported: Vec::new(),
                }
            })?,
        )
    } else {
        None
    };
    let refused =
        |reason: RuntimeFailure| ModelSwitchError::RuntimeControlRefused(reason.to_string());
    crate::async_runtime::block_on(async {
        let session = engine::ensure(svc, tab).await.map_err(refused)?;
        let Ok(permit) = tokio::time::timeout(GATE_WAIT, session.gate.acquire()).await else {
            return Err(ModelSwitchError::RuntimeBusy);
        };
        let _permit = permit.map_err(|_| refused(RuntimeFailure::Superseded))?;
        switch_inner(svc, tab, session.clone(), model, effort, mode, service_tier)
            .await
            .map_err(refused)
    })
}

async fn switch_inner(
    svc: &ServiceContext,
    tab: &str,
    session: std::sync::Arc<engine::Session>,
    model: &str,
    effort: Option<&str>,
    mode: Option<RuntimeMode>,
    service_tier: Option<codeterm_plugin::agent_runtime::RuntimeServiceTierSelection>,
) -> Result<ModelSwitchOutcome, RuntimeFailure> {
    engine::validate_binding(svc, &session)?;
    refresh_service_tiers(svc, &session).await?;
    let before = session.context.lock().clone();
    let thread = before.thread_id.clone().ok_or(RuntimeFailure::Superseded)?;
    let mut settings = before.settings.clone();
    if !model.trim().is_empty() {
        settings.model = Some(model.trim().into());
    }
    if let Some(effort) = effort {
        settings.effort = Some(effort.into());
    }
    if let Some(service_tier) = service_tier {
        let supported = svc
            .agent_registry
            .with_provider(&session.provider, |provider| {
                provider
                    .agent_runtime()
                    .is_some_and(|capability| !capability.service_tiers.is_empty())
            })
            .unwrap_or(false);
        if !supported {
            return Err(RuntimeFailure::Refused(
                "Service tiers are unsupported".into(),
            ));
        }
        if !tier_available(&settings, &service_tier) {
            return Err(RuntimeFailure::Refused(
                "Service tier is unavailable for this account and model".into(),
            ));
        }
        settings.service_tier = Some(service_tier);
    }
    if let Some(mode) = mode {
        settings.mode = serde_json::to_value(mode)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned));
    }
    let confirmed = if let Some(confirmed) = unchanged_settings(&settings, &before.settings) {
        confirmed
    } else {
        let operation = RuntimeOperation::Control {
            thread_id: Some(thread.clone()),
            action: "settings".into(),
            value: serde_json::to_value(&settings)
                .map_err(|reason| RuntimeFailure::Refused(reason.to_string()))?,
        };
        let mut changes = session.control_changes.subscribe();
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
        if !matches!(result, RuntimeResult::Done) {
            return Err(RuntimeFailure::Refused(
                "Runtime did not apply requested controls".into(),
            ));
        }
        if settings.service_tier.is_some() {
            tokio::time::timeout(std::time::Duration::from_secs(30), async {
                loop {
                    let confirmed = session.controls_confirmation.lock().take();
                    if let Some(confirmed) = confirmed {
                        return Ok::<_, RuntimeFailure>(confirmed);
                    }
                    changes
                        .changed()
                        .await
                        .map_err(|_| RuntimeFailure::PipeEof)?;
                    engine::validate_binding(svc, &session)?;
                }
            })
            .await
            .map_err(|_| {
                RuntimeFailure::Refused("Runtime settings confirmation deadline".into())
            })??
        } else {
            session.controls_confirmation.lock().take().ok_or_else(|| {
                RuntimeFailure::Refused("Runtime did not confirm requested controls".into())
            })?
        }
    };
    engine::validate_binding(svc, &session)?;
    if session.context.lock().epoch != before.epoch {
        return Err(RuntimeFailure::Superseded);
    }
    if !confirms(&settings, &confirmed) {
        return Err(RuntimeFailure::Refused(
            "Runtime did not confirm requested controls".into(),
        ));
    }
    let actual_model = confirmed
        .model
        .clone()
        .ok_or_else(|| RuntimeFailure::Refused("Runtime model confirmation absent".into()))?;
    let persisted_mode = confirmed
        .mode
        .clone()
        .filter(|_| before.settings.modes.is_none() && confirmed.modes.is_none());
    {
        let mut mux = svc.mux.write();
        let target = mux.get_tab_mut(tab).ok_or(RuntimeFailure::Superseded)?;
        if target.agent_type.as_deref() != Some(session.provider.as_str())
            || target.session_binding_nonce.as_deref() != Some(before.binding_nonce.as_str())
            || target.agent_session_id.as_deref() != Some(thread.as_str())
        {
            return Err(RuntimeFailure::Superseded);
        }
        target.restorable_launch.structured_runtime_mode = persisted_mode.clone();
        if let (Some(options), Some(mode)) = (
            target.restorable_launch.runtime_launch_options.as_mut(),
            persisted_mode.as_ref(),
        ) {
            options.mode = mode.clone();
        }
        target.pending_model_selection = None;
        target.pending_model_switch_observation = None;
        let config = target
            .agent_runtime_config
            .get_or_insert_with(|| serde_json::json!({}));
        if let Some(value) = &persisted_mode {
            config["mode"] = value.clone().into();
        }
    }
    session.state.lock().notice = None;
    engine::publish_controls(svc, tab, &session);
    super::selections::persist(svc, &session, &before, &settings, &confirmed)?;
    svc.emit_state_changed(MuxEmitCause::RuntimeSignal);
    Ok(ModelSwitchOutcome {
        tab_id: tab.into(),
        provider: session.provider.clone(),
        session_id: thread,
        restart: None,
        mechanism: crate::agent::provider::ModelSwitchMode::Live,
        fallback_reason: None,
        control_changes: control_changes(&before.settings, &confirmed),
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

fn declared_value(settings: &RuntimeSettings, category: &str) -> Option<String> {
    settings
        .config_options
        .as_ref()?
        .iter()
        .find(|option| option.category.as_deref() == Some(category))
        .map(|option| option.current_value.clone())
}

/// Effort, mode and speed before and after a switch; a control the confirmation omits is unchanged.
pub(super) fn control_changes(
    before: &RuntimeSettings,
    after: &RuntimeSettings,
) -> Vec<crate::service::agent_restart::ControlChange> {
    let effort = |settings: &RuntimeSettings| {
        declared_value(settings, "thought_level").or_else(|| settings.effort.clone())
    };
    let mode = |settings: &RuntimeSettings| settings.effective_mode().map(str::to_owned);
    let speed = |settings: &RuntimeSettings| {
        settings
            .service_tier
            .as_ref()
            .and_then(|tier| tier.id.clone())
    };
    let effort_before = effort(before);
    let mode_before = mode(before);
    let speed_before = speed(before);
    [
        (
            "effort",
            effort(after).or_else(|| effort_before.clone()),
            effort_before,
        ),
        (
            "mode",
            mode(after).or_else(|| mode_before.clone()),
            mode_before,
        ),
        (
            "speed",
            if after.service_tier.is_some() {
                speed(after)
            } else {
                speed_before.clone()
            },
            speed_before,
        ),
    ]
    .into_iter()
    .map(|(control, current, previous)| (control, previous, current))
    .filter(|(_, previous, current)| previous != current)
    .map(
        |(control, previous, current)| crate::service::agent_restart::ControlChange {
            control,
            previous,
            current,
        },
    )
    .collect()
}

fn unchanged_settings(
    requested: &RuntimeSettings,
    current: &RuntimeSettings,
) -> Option<RuntimeSettings> {
    (current.model.is_some() && confirms(requested, current)).then(|| current.clone())
}

fn confirms(requested: &RuntimeSettings, confirmed: &RuntimeSettings) -> bool {
    requested
        .service_tier
        .as_ref()
        .is_none_or(|value| confirmed.service_tier.as_ref() == Some(value))
        && [
            (&requested.model, &confirmed.model),
            (&requested.effort, &confirmed.effort),
            (&requested.mode, &confirmed.mode),
        ]
        .into_iter()
        .all(|(requested, confirmed)| requested.is_none() || requested == confirmed)
}

fn tier_available(
    settings: &RuntimeSettings,
    selection: &codeterm_plugin::agent_runtime::RuntimeServiceTierSelection,
) -> bool {
    selection.id.as_ref().is_none_or(|id| {
        settings
            .service_tier_availability
            .as_ref()
            .and_then(|models| settings.model.as_ref().and_then(|model| models.get(model)))
            .is_some_and(|tiers| {
                tiers
                    .iter()
                    .any(|tier| &tier.id == id && tier.unavailable_reason.is_none())
            })
    })
}

pub(crate) async fn refresh_service_tiers(
    svc: &ServiceContext,
    session: &std::sync::Arc<engine::Session>,
) -> Result<(), RuntimeFailure> {
    let supported = svc
        .agent_registry
        .with_provider(&session.provider, |provider| {
            provider
                .agent_runtime()
                .is_some_and(|capability| !capability.service_tiers.is_empty())
        })
        .unwrap_or(false);
    if !supported {
        return Ok(());
    }
    super::projections::controls(
        svc,
        session,
        RuntimeSettings {
            service_tier_availability: Some(Default::default()),
            ..Default::default()
        },
    );
    let context = session.context.lock().clone();
    let process = session
        .process
        .lock()
        .clone()
        .ok_or(RuntimeFailure::PipeEof)?;
    let result = engine::run(
        svc,
        &session.provider,
        session,
        &process,
        RuntimeOperation::Control {
            thread_id: context.thread_id,
            action: "serviceTiers".into(),
            value: serde_json::json!({}),
        },
    )
    .await?;
    engine::validate_binding(svc, session)?;
    if !matches!(result, RuntimeResult::Done) {
        return Err(RuntimeFailure::Refused(
            "Service tier catalogue unavailable".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;

mod interrupt;
pub(crate) use interrupt::{before_first_turn_pending, InterruptDropped, InterruptOutcome};

/// How long a control or Stop waits for a running runtime operation to release the gate.
pub(super) const GATE_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

pub fn interrupt(svc: &ServiceContext, tab: &str) -> Result<bool, String> {
    interrupt::interrupt(svc, tab).map(|outcome| {
        matches!(
            outcome,
            InterruptOutcome::Stopped | InterruptOutcome::Pending
        )
    })
}

pub(crate) fn stop(svc: &ServiceContext, tab: &str) -> Result<InterruptOutcome, String> {
    interrupt::interrupt(svc, tab)
}

pub(super) fn observe_event(session: &engine::Session, event: &super::events::RuntimeEvent) {
    if let super::events::RuntimeEvent::ThreadReverted { thread_id } = event {
        if session.context.lock().thread_id.as_deref() == Some(thread_id) {
            *session.reverted_thread.lock() = Some(thread_id.clone());
            session.control_changes.send_replace(());
        }
    }
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
