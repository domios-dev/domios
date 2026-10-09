use std::sync::mpsc::{channel, Sender};
use std::time::{Duration, Instant};

use super::*;
use crate::multiplexer::types::{PendingModelSelection, Tab};

const STAGING_DEADLINE: Duration = Duration::from_secs(3600);

pub(in crate::service::agent_restart) struct StagedSwitch {
    pub(super) request: SwitchRequest,
    pub(super) idle: Sender<()>,
}

#[derive(Default)]
pub(in crate::service::agent_restart) struct PendingSwitches(
    std::collections::HashMap<String, StagedSwitch>,
);

impl PendingSwitches {
    fn replace(&mut self, request: SwitchRequest, idle: Sender<()>) {
        self.0
            .insert(request.tab_id.clone(), StagedSwitch { request, idle });
    }

    fn cancel(&mut self, tab_id: &str) {
        self.0.remove(tab_id);
    }

    fn signal_idle(&self, tab_id: &str) {
        if let Some(staged) = self.0.get(tab_id) {
            let _ = staged.idle.send(());
        }
    }

    fn claim(&mut self, request: &SwitchRequest) -> bool {
        if !self
            .0
            .get(&request.tab_id)
            .is_some_and(|staged| staged.request.id == request.id)
        {
            return false;
        }
        self.0.remove(&request.tab_id);
        true
    }
}

pub(super) struct StagingParams {
    pub mechanism: ModelSwitchMode,
    pub force: bool,
    pub restart_fallback: bool,
    pub previous_model: Option<String>,
}

#[derive(Clone)]
pub(super) struct SwitchRequest {
    id: String,
    tab_id: String,
    provider: String,
    session_id: String,
    pty_id: String,
    nonce: String,
    launch: RestorableLaunchParameters,
    model: String,
    effort: Option<String>,
    force: bool,
    restart_fallback: bool,
    deadline: Instant,
}

impl SwitchRequest {
    fn matches(&self, tab: &Tab, now: Instant) -> bool {
        now < self.deadline
            && tab.agent_type.as_deref() == Some(&self.provider)
            && tab.agent_session_id.as_deref() == Some(&self.session_id)
            && tab.pty_id.as_deref() == Some(&self.pty_id)
            && tab.session_binding_nonce.as_deref() == Some(&self.nonce)
            && tab.reported_session_binding_nonce.as_deref() == Some(&self.nonce)
            && tab.restorable_launch == self.launch
    }

    pub(super) fn require_current(&self, svc: &ServiceContext) -> Result<(), ModelSwitchError> {
        if svc
            .mux
            .read()
            .get_tab(&self.tab_id)
            .is_some_and(|tab| self.matches(tab, Instant::now()))
        {
            Ok(())
        } else {
            Err(ModelSwitchError::NotSwitchable("staged model switch refused: the launch or session binding changed, or its deadline elapsed".into()))
        }
    }
}

impl ServiceContext {
    pub(crate) fn cancel_staged_model_switch(&self, tab_id: &str) {
        let mut mux = self.mux.write();
        self.restart_tracker.lock().staged_models.cancel(tab_id);
        if let Some(tab) = mux.get_tab_mut(tab_id) {
            tab.pending_model_selection = None;
        }
        drop(mux);
        self.emit_state_changed();
    }

    pub(crate) fn apply_staged_model_switch_on_idle(&self, tab_id: &str) {
        self.restart_tracker
            .lock()
            .staged_models
            .signal_idle(tab_id);
    }

    pub(super) fn stage_model_switch(
        &self,
        snap: &TabRestartSnapshot,
        session_id: &str,
        model: &str,
        effort: Option<&str>,
        params: StagingParams,
    ) -> Result<ModelSwitchOutcome, ModelSwitchError> {
        let (tx, rx) = channel();
        let request = {
            let mut mux = self.mux.write();
            let tab = mux
                .get_tab_mut(&snap.tab_id)
                .ok_or_else(|| ModelSwitchError::NotFound(snap.tab_id.clone()))?;
            let nonce = tab
                .session_binding_nonce
                .clone()
                .filter(|nonce| tab.reported_session_binding_nonce.as_ref() == Some(nonce))
                .ok_or_else(|| {
                    ModelSwitchError::NotSwitchable(
                        "no verified session binding for staging".into(),
                    )
                })?;
            if tab.pty_id.as_deref() != Some(&snap.pty_id)
                || tab.agent_session_id.as_deref() != Some(session_id)
                || tab.agent_type.as_deref() != Some(&snap.provider)
            {
                return Err(ModelSwitchError::NotSwitchable(
                    "the session changed before staging".into(),
                ));
            }
            let request = SwitchRequest {
                id: uuid::Uuid::new_v4().to_string(),
                tab_id: snap.tab_id.clone(),
                provider: snap.provider.clone(),
                session_id: session_id.into(),
                pty_id: snap.pty_id.clone(),
                nonce,
                launch: tab.restorable_launch.clone(),
                model: model.into(),
                effort: effort.map(str::to_owned),
                force: params.force,
                restart_fallback: params.restart_fallback,
                deadline: Instant::now() + STAGING_DEADLINE,
            };
            tab.pending_model_selection = Some(PendingModelSelection {
                model: model.into(),
                effort: effort.map(str::to_owned),
            });
            self.restart_tracker
                .lock()
                .staged_models
                .replace(request.clone(), tx);
            request
        };
        let svc = self.clone();
        let worker_request = request.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("model-switch-idle".into())
            .spawn(move || {
                let idle = rx
                    .recv_timeout(
                        worker_request
                            .deadline
                            .saturating_duration_since(Instant::now()),
                    )
                    .is_ok();
                svc.settle_staged_model_switch(worker_request, idle);
            })
        {
            self.settle_staged_model_switch(request, false);
            return Err(ModelSwitchError::NotSwitchable(format!(
                "could not stage model switch: {error}"
            )));
        }
        self.emit_state_changed();
        // Recheck after subscribing so an idle edge during staging cannot be lost.
        if self.tab_is_idle(&snap.tab_id) {
            self.apply_staged_model_switch_on_idle(&snap.tab_id);
        }
        Ok(ModelSwitchOutcome {
            tab_id: snap.tab_id.clone(),
            provider: snap.provider.clone(),
            session_id: session_id.into(),
            restart: None,
            mechanism: params.mechanism,
            fallback_reason: None,
            previous_model: params.previous_model,
            requested_model: model.into(),
            launched_model: model.into(),
            confirmation_detail: None,
            confirmation: ModelSwitchConfirmation::AfterTurn,
        })
    }

    fn settle_staged_model_switch(&self, request: SwitchRequest, idle: bool) {
        {
            let mut mux = self.mux.write();
            let mut tracker = self.restart_tracker.lock();
            if !tracker.staged_models.claim(&request) {
                return;
            }
            if let Some(tab) = mux.get_tab_mut(&request.tab_id) {
                tab.pending_model_selection = None;
            }
        }
        let result = if !idle {
            Err(ModelSwitchError::NotSwitchable(
                "staged model switch deadline elapsed".into(),
            ))
        } else {
            request.require_current(self).and_then(|()| {
                self.execute_model_switch(
                    &request.tab_id,
                    &request.model,
                    request.effort.as_deref(),
                    request.force,
                    request.restart_fallback,
                    Some(&request),
                )
            })
        };
        if let Err(error) = result {
            self.create_notification_ext(
                "Model switch refused".into(),
                Some(format!("Tab {}: {error}", request.tab_id)),
                Some("warning"),
                None,
                Some(false),
                Default::default(),
                Default::default(),
                None,
            );
        }
        self.emit_state_changed();
    }
}

#[cfg(test)]
mod tests;
