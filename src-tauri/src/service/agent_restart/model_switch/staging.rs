use std::sync::mpsc::{channel, Sender};
use std::time::{Duration, Instant};

use super::*;
use crate::multiplexer::types::{PendingModelSelection, Tab};

const STAGING_DEADLINE: Duration = Duration::from_secs(3600);
mod completion_records;

pub(in crate::service::agent_restart) struct StagedSwitch {
    pub(super) request: SwitchRequest,
    pub(super) idle: Sender<()>,
}

#[derive(Default)]
pub(in crate::service::agent_restart) struct PendingSwitches(
    std::collections::HashMap<String, StagedSwitch>,
    std::collections::HashMap<
        String,
        std::sync::Weak<crate::pty::screen_state::ScreenChangeSignal>,
    >,
    std::collections::HashMap<String, (String, String, u64)>,
);

impl PendingSwitches {
    fn completion_generation(&self, tab: &str, session: &str, nonce: &str) -> u64 {
        self.2
            .get(tab)
            .filter(|(own_session, own_nonce, _)| own_session == session && own_nonce == nonce)
            .map_or(0, |(_, _, generation)| *generation)
    }

    fn completed(&self, request: &SwitchRequest) -> bool {
        let request = self
            .0
            .get(&request.tab_id)
            .filter(|staged| staged.request.id == request.id)
            .map_or(request, |staged| &staged.request);
        self.completion_generation(&request.tab_id, &request.session_id, &request.nonce)
            > request.completion_generation
    }

    fn signal_started(&mut self, tab_id: &str) {
        if let Some(staged) = self.0.get(tab_id) {
            let generation = self.completion_generation(
                tab_id,
                &staged.request.session_id,
                &staged.request.nonce,
            );
            if let Some(staged) = self.0.get_mut(tab_id) {
                staged.request.completion_generation = generation;
            }
        }
    }

    fn signal_completed(&mut self, tab: &str, session: &str, nonce: &str) {
        let generation = self
            .completion_generation(tab, session, nonce)
            .saturating_add(1);
        self.2
            .insert(tab.into(), (session.into(), nonce.into(), generation));
        self.signal_idle(tab);
    }
    fn replace(&mut self, request: SwitchRequest, idle: Sender<()>) {
        if let Some(previous) = self
            .0
            .insert(request.tab_id.clone(), StagedSwitch { request, idle })
        {
            let _ = previous.idle.send(());
        }
    }

    fn replace_at_idle(&mut self, request: SwitchRequest, idle: Sender<()>, already_idle: bool) {
        let tab_id = request.tab_id.clone();
        self.replace(request, idle);
        if already_idle {
            self.signal_idle(&tab_id);
        }
    }

    fn cancel(&mut self, tab_id: &str) {
        if let Some(previous) = self.0.remove(tab_id) {
            let _ = previous.idle.send(());
        }
    }

    fn signal_idle(&self, tab_id: &str) {
        if let Some(signal) = self.1.get(tab_id).and_then(std::sync::Weak::upgrade) {
            signal.mark_changed();
        }
        if let Some(staged) = self
            .0
            .get(tab_id)
            .filter(|staged| self.completed(&staged.request))
        {
            let _ = staged.idle.send(());
        }
    }

    pub(super) fn observe_command_idle(
        &mut self,
        tab_id: &str,
        signal: &std::sync::Arc<crate::pty::screen_state::ScreenChangeSignal>,
    ) {
        self.1.retain(|_, signal| signal.strong_count() > 0);
        self.1
            .insert(tab_id.into(), std::sync::Arc::downgrade(signal));
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
    pub completion_boundary: CompletionBoundary,
}

pub(super) struct CompletionBoundary {
    session_id: String,
    nonce: String,
    generation: u64,
    records: Option<completion_records::RecordBoundary>,
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
    mechanism: ModelSwitchMode,
    deadline: Instant,
    completion_generation: u64,
}

impl SwitchRequest {
    pub(super) fn require_mechanism(
        &self,
        mechanism: ModelSwitchMode,
    ) -> Result<(), ModelSwitchError> {
        if self.mechanism == mechanism {
            Ok(())
        } else {
            Err(ModelSwitchError::NotSwitchable(
                "staged model switch refused: provider switch capability changed".into(),
            ))
        }
    }
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

    pub(super) fn require_snapshot(
        &self,
        snap: &TabRestartSnapshot,
    ) -> Result<(), ModelSwitchError> {
        if snap.tab_id == self.tab_id
            && snap.pty_id == self.pty_id
            && snap.provider == self.provider
            && snap.session_id.as_deref() == Some(&self.session_id)
        {
            Ok(())
        } else {
            Err(ModelSwitchError::NotSwitchable("staged model switch refused: restart snapshot belongs to another launch or session".into()))
        }
    }
}

impl ServiceContext {
    pub(crate) fn staged_model_turn_started(&self, tab_id: &str) {
        self.restart_tracker
            .lock()
            .staged_models
            .signal_started(tab_id);
    }
    pub(super) fn staged_completion_boundary(&self, tab_id: &str) -> Option<CompletionBoundary> {
        let (session_id, nonce, generation, provider, cwd) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id)?;
            let session_id = tab.agent_session_id.clone()?;
            let nonce = tab.session_binding_nonce.clone()?;
            if tab.reported_session_binding_nonce.as_ref() != Some(&nonce) {
                return None;
            }
            let generation = self
                .restart_tracker
                .lock()
                .staged_models
                .completion_generation(tab_id, &session_id, &nonce);
            (
                session_id,
                nonce,
                generation,
                tab.agent_type.clone()?,
                tab.cwd.clone(),
            )
        };
        let declared = self
            .agent_registry
            .with_provider(&provider, |provider| {
                provider
                    .model_switch_commands()
                    .and_then(|commands| commands.turn_completion)
                    .map(|declaration| {
                        (
                            cwd.as_deref().and_then(|cwd| {
                                declaration.record_path(provider, cwd, &session_id)
                            }),
                            declaration,
                        )
                    })
            })
            .flatten();
        let records = match declared {
            Some((path, declaration)) => {
                let path = path?;
                let records = completion_records::RecordBoundary::capture(
                    path.clone(),
                    declaration.clone(),
                    &session_id,
                )?;
                log::info!("[agent-model/completion] tab={tab_id} source={:?} path={} phase=boundary_captured", declaration.source, path.display());
                Some(records)
            }
            None => None,
        };
        Some(CompletionBoundary {
            session_id,
            nonce,
            generation,
            records,
        })
    }

    pub(crate) fn staged_model_turn_completed(&self, tab_id: &str, session_id: &str, nonce: &str) {
        let mux = self.mux.read();
        let accepted = mux.get_tab(tab_id).is_some_and(|tab| {
            tab.agent_session_id.as_deref() == Some(session_id)
                && tab.session_binding_nonce.as_deref() == Some(nonce)
                && tab.reported_session_binding_nonce.as_deref() == Some(nonce)
        });
        log::info!("[agent-model/completion] tab={tab_id} phase=staged_model_turn_completed accepted={accepted}");
        if accepted {
            self.restart_tracker
                .lock()
                .staged_models
                .signal_completed(tab_id, session_id, nonce);
        }
    }
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
        let record_wake = tx.clone();
        let mut records = params.completion_boundary.records;
        let watcher = records
            .as_ref()
            .map(|records| records.watch(tx.clone()))
            .transpose()
            .map_err(ModelSwitchError::NotSwitchable)?;
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
                || params.completion_boundary.session_id != session_id
                || params.completion_boundary.nonce != nonce
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
                mechanism: params.mechanism,
                deadline: Instant::now() + STAGING_DEADLINE,
                completion_generation: params.completion_boundary.generation,
            };
            tab.pending_model_selection = Some(PendingModelSelection {
                model: model.into(),
                effort: effort.map(str::to_owned),
            });
            let already_idle =
                self.resolved_tab_status(tab) == Some(codeterm_types::pty::TabStatus::Idle);
            self.restart_tracker.lock().staged_models.replace_at_idle(
                request.clone(),
                tx,
                already_idle,
            );
            request
        };
        let svc = self.clone();
        let worker_request = request.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("model-switch-idle".into())
            .spawn(move || {
                let _watcher = watcher;
                loop {
                    let current = svc
                        .restart_tracker
                        .lock()
                        .staged_models
                        .0
                        .get(&worker_request.tab_id)
                        .is_some_and(|staged| staged.request.id == worker_request.id);
                    if !current {
                        break;
                    }
                    if worker_request.require_current(&svc).is_err() {
                        svc.settle_staged_model_switch(worker_request, false);
                        break;
                    }
                    if let Some(records) = &mut records {
                        let completed = records.completed(&worker_request.session_id);
                        if records.started() {
                            svc.staged_model_turn_started(&worker_request.tab_id);
                        }
                        match completed {
                            Ok(true) => {
                                log::info!("[agent-model/completion] tab={} phase=matching_record_observed", worker_request.tab_id);
                                svc.staged_model_turn_completed(
                                &worker_request.tab_id,
                                &worker_request.session_id,
                                &worker_request.nonce,
                                );
                            }
                            Ok(false) => {}
                            Err(error) => {
                                log::warn!("[agent-model/completion] tab={} phase=record_read_failed error={error}", worker_request.tab_id);
                                svc.settle_staged_model_switch(worker_request, false);
                                break;
                            }
                        }
                        if records.has_more() {
                            let _ = record_wake.send(());
                        }
                    }
                    let idle = rx
                        .recv_timeout(
                            worker_request
                                .deadline
                                .saturating_duration_since(Instant::now()),
                        )
                        .is_ok();
                    let completed = svc
                        .restart_tracker
                        .lock()
                        .staged_models
                        .completed(&worker_request);
                    if idle
                        && worker_request.require_current(&svc).is_ok()
                        && (!completed
                            || !staged_tab_idle(
                                &svc,
                                &worker_request.tab_id,
                                worker_request.deadline,
                            ))
                    {
                        continue;
                    }
                    svc.settle_staged_model_switch(worker_request, idle);
                    break;
                }
            })
        {
            self.settle_staged_model_switch(request, false);
            return Err(ModelSwitchError::NotSwitchable(format!(
                "could not stage model switch: {error}"
            )));
        }
        self.emit_state_changed();
        // Recheck after subscribing so an idle edge during staging cannot be lost.
        if staged_tab_idle(
            self,
            &snap.tab_id,
            Instant::now() + Duration::from_secs(MODEL_OBSERVE_TIMEOUT_SECS),
        ) {
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

fn staged_tab_idle(svc: &ServiceContext, tab_id: &str, deadline: Instant) -> bool {
    crate::service::status_reconciliation::snapshot_with_reconciled_status_until(
        &svc.mux,
        &svc.agent_tracker,
        &svc.agent_registry,
        deadline,
    )
    .is_some_and(|snapshot| {
        snapshot.tab_statuses.get(tab_id) == Some(&codeterm_types::pty::TabStatus::Idle)
    })
}

#[cfg(test)]
mod tests;
