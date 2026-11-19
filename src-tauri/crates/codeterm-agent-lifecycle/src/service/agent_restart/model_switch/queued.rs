use std::sync::Arc;

use super::{staging::STAGING_DEADLINE, *};
use crate::multiplexer::types::PendingModelSelection;
use crate::service::agent_lifecycle::{QueuedModelSwitch, RestartTracker};
use crate::service::agent_runtime::{engine, restart::model_restart_busy};
use crate::service::boot_wait;
use tokio::sync::Notify;

pub(super) fn request(
    svc: &ServiceContext,
    tab_id: &str,
    model: &str,
    effort: Option<&str>,
    force: bool,
    restart_fallback: bool,
) -> Result<ModelSwitchOutcome, ModelSwitchError> {
    let tracker = svc.ext::<RestartTracker>();
    let claim = {
        let mut mux = svc.mux.write();
        let tab_id = mux
            .resolve_tab_id(tab_id)
            .ok_or_else(|| ModelSwitchError::NotFound(tab_id.into()))?;
        let tab = mux
            .get_tab_mut(&tab_id)
            .ok_or_else(|| ModelSwitchError::NotFound(tab_id.clone()))?;
        let mut state = tracker.lock();
        let restart_target = state.restart_targets.get(&tab_id);
        let provider = tab
            .agent_type
            .as_ref()
            .or_else(|| restart_target.map(|target| &target.0));
        let mode = provider.and_then(|provider| {
            svc.agent_registry
                .with_provider(provider, |p| p.model_switch())
        });
        let restart_based = matches!(
            mode,
            Some(ModelSwitchMode::RestartResume | ModelSwitchMode::Live)
        ) || (restart_fallback && mode == Some(ModelSwitchMode::LiveCommand));
        if !restart_based {
            None
        } else if !state.model_switch_busy(&tab_id)
            && !state.queued_model_switches.contains_key(&tab_id)
        {
            state.running_model_switches.insert(tab_id.clone(), None);
            Some(ModelSwitchClaim { svc, tab_id })
        } else {
            let provider = provider
                .cloned()
                .ok_or_else(|| ModelSwitchError::NotFound(tab_id.clone()))?;
            let previous_model = tab
                .agent_session_model
                .clone()
                .or_else(|| tab.agent_provider_resolved_model.clone());
            let requested_model = model.trim();
            let model = if requested_model.is_empty() {
                tab.agent_runtime_config
                    .as_ref()
                    .and_then(|config| config["model"].as_str())
                    .or(tab.restorable_launch.model.as_deref())
                    .or(previous_model.as_deref())
                    .unwrap_or(requested_model)
            } else {
                requested_model
            };
            let selection = PendingModelSelection {
                model: model.to_owned(),
                effort: effort.map(str::to_owned),
                confirmation_pending: false,
            };
            let session_id = tab
                .agent_session_id
                .clone()
                .or_else(|| restart_target.and_then(|target| target.1.clone()));
            let cancelled = if let Some(pending) = state.queued_model_switches.get(&tab_id) {
                pending.cancelled.clone()
            } else {
                let cancelled = Arc::new(Notify::new());
                let worker_cancelled = cancelled.clone();
                let worker_svc = svc.clone();
                let worker_tab = tab_id.clone();
                std::thread::Builder::new()
                    .name("model-switch-restart".into())
                    .spawn(move || drain(&worker_svc, &worker_tab, &worker_cancelled))
                    .map_err(|error| {
                        ModelSwitchError::NotSwitchable(format!(
                            "could not queue model switch: {error}"
                        ))
                    })?;
                cancelled
            };
            state.queued_model_switches.insert(
                tab_id.clone(),
                QueuedModelSwitch {
                    provider: provider.clone(),
                    session_id: session_id.clone(),
                    force,
                    restart_fallback,
                    requested_model: requested_model.to_owned(),
                    selection: selection.clone(),
                    cancelled,
                },
            );
            tab.pending_model_selection = Some(selection.clone());
            drop(state);
            drop(mux);
            boot_wait::notify_boot_changed();
            svc.emit_state_changed(MuxEmitCause::Model);
            return Ok(ModelSwitchOutcome {
                tab_id,
                provider,
                session_id: session_id.unwrap_or_default(),
                restart: None,
                mechanism: if restart_fallback && mode == Some(ModelSwitchMode::LiveCommand) {
                    ModelSwitchMode::RestartResume
                } else {
                    mode.unwrap_or(ModelSwitchMode::RestartResume)
                },
                fallback_reason: None,
                previous_model,
                requested_model: selection.model.clone(),
                launched_model: selection.model,
                confirmation_detail: None,
                confirmation: ModelSwitchConfirmation::AfterTurn,
                control_changes: Vec::new(),
            });
        }
    };
    let target = claim.as_ref().map_or(tab_id, |claim| claim.tab_id.as_str());
    svc.execute_model_switch(target, model, effort, force, restart_fallback, None)
}

struct ModelSwitchClaim<'a> {
    svc: &'a ServiceContext,
    tab_id: String,
}

impl Drop for ModelSwitchClaim<'_> {
    fn drop(&mut self) {
        let mut mux = self.svc.mux.write();
        let mut tracker = self.svc.ext::<RestartTracker>().lock();
        let selection = tracker
            .running_model_switches
            .remove(&self.tab_id)
            .flatten();
        let mut changed = false;
        if let Some(selection) = selection {
            if let Some(tab) = mux.get_tab_mut(&self.tab_id) {
                if !tracker.queued_model_switches.contains_key(&self.tab_id)
                    && !tracker.staged_models.0.contains_key(&self.tab_id)
                    && tab.pending_model_selection.as_ref().is_some_and(|pending| {
                        !pending.confirmation_pending
                            && pending.model == selection.model
                            && pending.effort == selection.effort
                    })
                {
                    tab.pending_model_selection = None;
                    changed = true;
                }
            }
        }
        tracker.staged_models.signal_idle(&self.tab_id);
        drop(tracker);
        drop(mux);
        boot_wait::notify_boot_changed();
        if changed {
            self.svc.emit_state_changed(MuxEmitCause::Model);
        }
    }
}

fn own_request<'a>(
    tracker: &'a super::super::RestartTrackerState,
    tab_id: &str,
    cancelled: &Arc<Notify>,
) -> Option<&'a QueuedModelSwitch> {
    tracker
        .queued_model_switches
        .get(tab_id)
        .filter(|pending| Arc::ptr_eq(&pending.cancelled, cancelled))
}

fn drain(svc: &ServiceContext, tab_id: &str, cancelled: &Arc<Notify>) {
    let tracker = svc.ext::<RestartTracker>();
    let deadline = Instant::now() + STAGING_DEADLINE;
    loop {
        if !boot_wait::wait_until(deadline, || {
            let state = tracker.lock();
            own_request(&state, tab_id, cancelled).is_none() || !state.model_switch_busy(tab_id)
        }) || !wait_for_runtime_idle(tab_id, cancelled, deadline)
        {
            expire(svc, tab_id, cancelled);
            return;
        }
        let (claim, pending) = {
            let mut state = tracker.lock();
            if own_request(&state, tab_id, cancelled).is_none() {
                return;
            }
            if state.model_switch_busy(tab_id) {
                continue;
            }
            let Some(pending) = state.queued_model_switches.remove(tab_id) else {
                return;
            };
            state
                .running_model_switches
                .insert(tab_id.into(), Some(pending.selection.clone()));
            (
                ModelSwitchClaim {
                    svc,
                    tab_id: tab_id.into(),
                },
                pending,
            )
        };
        let nonce = {
            let mux = svc.mux.read();
            mux.get_tab(tab_id)
                .filter(|tab| {
                    tab.agent_type.as_deref() == Some(&pending.provider)
                        && pending
                            .session_id
                            .as_ref()
                            .is_none_or(|id| tab.agent_session_id.as_ref() == Some(id))
                })
                .and_then(|tab| tab.session_binding_nonce.clone())
        };
        let result = if nonce.is_some() {
            svc.execute_model_switch(
                tab_id,
                &pending.requested_model,
                pending.selection.effort.as_deref(),
                pending.force,
                pending.restart_fallback,
                None,
            )
        } else {
            Err(ModelSwitchError::NotSwitchable(
                "the queued model switch's provider or session changed".into(),
            ))
        };
        svc.record_product_model_switch(&result, nonce.as_deref());
        drop(claim);
        if let Err(error) = result {
            svc.log_system_notice(
                "Model switch did not apply".into(),
                Some(format!("Tab {tab_id}: {error}")),
                Some("warning"),
            );
        }
        return;
    }
}

fn wait_for_runtime_idle(tab_id: &str, cancelled: &Notify, deadline: Instant) -> bool {
    let Some(session) = engine::for_tab(tab_id) else {
        return true;
    };
    let mut changes = session.control_changes.subscribe();
    if !model_restart_busy(&session) {
        return true;
    }
    crate::async_runtime::block_on(async {
        tokio::time::timeout(deadline.saturating_duration_since(Instant::now()), async {
            while model_restart_busy(&session)
                && engine::for_tab(tab_id).is_some_and(|current| Arc::ptr_eq(&current, &session))
            {
                tokio::select! {
                    result = changes.changed() => { if result.is_err() { return false; } }
                    _ = cancelled.notified() => return false,
                }
            }
            true
        })
        .await
        .unwrap_or(false)
    })
}

fn expire(svc: &ServiceContext, tab_id: &str, cancelled: &Arc<Notify>) {
    let mut mux = svc.mux.write();
    let mut tracker = svc.ext::<RestartTracker>().lock();
    if own_request(&tracker, tab_id, cancelled).is_none() {
        return;
    }
    tracker.queued_model_switches.remove(tab_id);
    if let Some(tab) = mux.get_tab_mut(tab_id) {
        tab.pending_model_selection = tracker.pending_restart_model(tab_id).cloned();
    }
    drop(tracker);
    drop(mux);
    svc.emit_state_changed(MuxEmitCause::Model);
    svc.log_system_notice(
        "Model switch did not apply".into(),
        Some(format!("Tab {tab_id}: the queued model switch expired")),
        Some("warning"),
    );
}
