use std::sync::mpsc::{channel, Sender};
use std::time::{Duration, Instant};

use super::*;
use crate::multiplexer::types::{PendingModelSelection, Tab};

const STAGING_DEADLINE: Duration = Duration::from_secs(3600);
mod completion_records;

pub(in crate::service::agent_restart) struct StagedSwitch {
    pub(super) request: SwitchRequest,
    pub(super) idle: Sender<()>,
    claimed: bool,
    warned: bool,
}

struct LatchedCompletion {
    generation: u64,
    nonce: String,
    anchor: crate::service::chat_send_transaction::SendEvidenceAnchor,
}

#[derive(Default)]
pub(in crate::service::agent_restart) struct PendingSwitches(
    std::collections::HashMap<String, StagedSwitch>,
    std::collections::HashMap<
        String,
        std::sync::Weak<crate::pty::screen_state::ScreenChangeSignal>,
    >,
    std::collections::HashMap<String, (String, String, u64)>,
    std::collections::HashMap<String, LatchedCompletion>,
    std::collections::HashMap<String, u64>,
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
        self.3.remove(tab_id);
        *self.4.entry(tab_id.into()).or_default() += 1;
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
    fn boundary_generation(
        &self,
        tab: &str,
        session: &str,
        nonce: &str,
        no_foreign_turn: bool,
    ) -> u64 {
        let generation = self.completion_generation(tab, session, nonce);
        if no_foreign_turn
            && self.3.get(tab).is_some_and(|completed| {
                completed.generation == generation
                    && completed.nonce == nonce
                    && completed.anchor.session_id == session
            })
        {
            generation.saturating_sub(1)
        } else {
            generation
        }
    }

    fn start_epoch(&self, tab: &str) -> u64 {
        self.4.get(tab).copied().unwrap_or_default()
    }

    fn latch_completed(
        &mut self,
        tab: &str,
        nonce: &str,
        epoch: u64,
        anchor: crate::service::chat_send_transaction::SendEvidenceAnchor,
    ) {
        if self.start_epoch(tab) != epoch {
            return;
        }
        let generation = self.completion_generation(tab, &anchor.session_id, nonce);
        self.3.insert(
            tab.into(),
            LatchedCompletion {
                generation,
                nonce: nonce.into(),
                anchor,
            },
        );
    }

    fn replace(&mut self, mut request: SwitchRequest, idle: Sender<()>) {
        let generation =
            self.completion_generation(&request.tab_id, &request.session_id, &request.nonce);
        if self.start_epoch(&request.tab_id) != request.completion_epoch {
            request.completion_generation = generation;
            request.completion_epoch = self.start_epoch(&request.tab_id);
        }
        self.signal_idle(&request.tab_id);
        if let Some(previous) = self.0.insert(
            request.tab_id.clone(),
            StagedSwitch {
                request,
                idle,
                claimed: false,
                warned: false,
            },
        ) {
            let _ = previous.idle.send(());
        }
    }

    fn replace_at_idle(&mut self, request: SwitchRequest, idle: Sender<()>, already_idle: bool) {
        let tab_id = request.tab_id.clone();
        self.replace(request, idle);
        if already_idle
            || self
                .0
                .get(&tab_id)
                .is_some_and(|staged| self.completed(&staged.request))
        {
            self.signal_idle(&tab_id);
        }
    }

    fn cancel(&mut self, tab_id: &str) {
        self.signal_idle(tab_id);
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
        let Some(staged) = self
            .0
            .get_mut(&request.tab_id)
            .filter(|staged| staged.request.id == request.id && !staged.claimed)
        else {
            return false;
        };
        staged.claimed = true;
        true
    }

    fn finish(&mut self, request: &SwitchRequest) {
        if self
            .0
            .get(&request.tab_id)
            .is_some_and(|staged| staged.request.id == request.id && staged.claimed)
        {
            self.0.remove(&request.tab_id);
        }
    }

    fn release(&mut self, request: &SwitchRequest) -> bool {
        let Some(staged) = self
            .0
            .get_mut(&request.tab_id)
            .filter(|staged| staged.request.id == request.id && staged.claimed)
        else {
            return false;
        };
        staged.claimed = false;
        true
    }

    fn warn_once(&mut self, request: &SwitchRequest) -> bool {
        self.0
            .get_mut(&request.tab_id)
            .filter(|staged| staged.request.id == request.id)
            .is_none_or(|staged| !std::mem::replace(&mut staged.warned, true))
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
    epoch: u64,
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
    completion_epoch: u64,
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
        let (session_id, nonce, mut generation, epoch, provider, cwd, completed_anchor) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id)?;
            let session_id = tab.agent_session_id.clone()?;
            let nonce = tab.session_binding_nonce.clone()?;
            if tab.reported_session_binding_nonce.as_ref() != Some(&nonce) {
                return None;
            }
            let tracker = self.restart_tracker.lock();
            let generation =
                tracker
                    .staged_models
                    .completion_generation(tab_id, &session_id, &nonce);
            let completed_anchor = tracker
                .staged_models
                .3
                .get(tab_id)
                .filter(|completed| {
                    completed.generation == generation
                        && completed.nonce == nonce
                        && completed.anchor.session_id == session_id
                })
                .map(|completed| completed.anchor.clone());
            (
                session_id,
                nonce,
                generation,
                tracker.staged_models.start_epoch(tab_id),
                tab.agent_type.clone()?,
                tab.cwd.clone(),
                completed_anchor,
            )
        };
        if let Some(anchor) = completed_anchor {
            let transcript = self
                .agent_registry
                .with_provider(&provider, |provider| {
                    provider
                        .model_switch_commands()
                        .map(|commands| commands.transcript)
                })
                .flatten();
            let no_foreign_turn = transcript.is_some_and(|transcript| {
                crate::service::chat_send_evidence::window_has_foreign_user_turn(
                    &self.agent_registry,
                    &anchor,
                    &transcript,
                ) == Ok(false)
            });
            generation = self
                .restart_tracker
                .lock()
                .staged_models
                .boundary_generation(tab_id, &session_id, &nonce, no_foreign_turn);
            if no_foreign_turn {
                log::info!(
                    "[agent-model/completion] tab={tab_id} phase=accepted_completion_latched"
                );
            }
        }
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
            epoch,
            records,
        })
    }

    pub(crate) fn staged_model_turn_completed(&self, tab_id: &str, session_id: &str, nonce: &str) {
        let accepted = self.mux.read().get_tab(tab_id).is_some_and(|tab| {
            tab.agent_session_id.as_deref() == Some(session_id)
                && tab.session_binding_nonce.as_deref() == Some(nonce)
                && tab.reported_session_binding_nonce.as_deref() == Some(nonce)
        });
        log::info!("[agent-model/completion] tab={tab_id} phase=staged_model_turn_completed accepted={accepted} at_ms={}", crate::service::pty_frame_timing::unix_ms());
        if accepted {
            let epoch = self
                .restart_tracker
                .lock()
                .staged_models
                .start_epoch(tab_id);
            let reading = Instant::now();
            let anchor = completion_anchor(self, tab_id, session_id);
            log::info!(
                "[agent-model/completion] tab={tab_id} phase=completion_anchor_read elapsed_ms={}",
                reading.elapsed().as_millis()
            );
            let mux = self.mux.read();
            if mux.get_tab(tab_id).is_some_and(|tab| {
                tab.agent_session_id.as_deref() == Some(session_id)
                    && tab.session_binding_nonce.as_deref() == Some(nonce)
                    && tab.reported_session_binding_nonce.as_deref() == Some(nonce)
            }) {
                let mut tracker = self.restart_tracker.lock();
                if tracker.staged_models.start_epoch(tab_id) != epoch {
                    return;
                }
                tracker
                    .staged_models
                    .signal_completed(tab_id, session_id, nonce);
                if let Some(anchor) = anchor.filter(|anchor| anchor.session_id == session_id) {
                    tracker
                        .staged_models
                        .latch_completed(tab_id, nonce, epoch, anchor);
                }
            }
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
                completion_epoch: params.completion_boundary.epoch,
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
                let frames = StagedFrames::new(&svc, &worker_request.pty_id);
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
                    log::info!("[agent-model/completion] tab={} request={} phase=staged_wake signaled={} completed={} at_ms={}", worker_request.tab_id, worker_request.id, idle, completed, crate::service::pty_frame_timing::unix_ms());
                    if idle
                        && worker_request.require_current(&svc).is_ok()
                        && (!completed
                            || (!svc.agent_registry.with_provider(&worker_request.provider, |provider| {
                                worker_request.mechanism == ModelSwitchMode::LiveCommand
                                    && provider.model_switch_commands().is_some_and(|commands| commands.ready_input_regex.is_some())
                            }).unwrap_or(false)
                                && !staged_tab_idle(&svc, &worker_request.tab_id, worker_request.deadline)))
                    {
                        continue;
                    }
                    let generation = frames.signal.generation();
                    if svc.settle_staged_model_switch(worker_request.clone(), idle) {
                        break;
                    }
                    {
                        let mut tracker = svc.restart_tracker.lock();
                        if !tracker.staged_models.0.get(&worker_request.tab_id)
                            .is_some_and(|staged| staged.request.id == worker_request.id) { break; }
                        tracker.staged_models.observe_command_idle(&worker_request.tab_id, &frames.signal);
                    }
                    log::info!("[agent-model/completion] tab={} request={} phase=staged_retry_wait", worker_request.tab_id, worker_request.id);
                    if !frames.signal.wait_after(generation, worker_request.deadline.saturating_duration_since(Instant::now())) {
                        svc.settle_staged_model_switch(worker_request, false);
                        break;
                    }
                    let _ = record_wake.send(());
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

    fn settle_staged_model_switch(&self, request: SwitchRequest, idle: bool) -> bool {
        log::info!(
            "[agent-model/completion] tab={} request={} phase=staged_claim_begin idle={} at_ms={}",
            request.tab_id,
            request.id,
            idle,
            crate::service::pty_frame_timing::unix_ms()
        );
        {
            let mut tracker = self.restart_tracker.lock();
            if !tracker.staged_models.claim(&request) {
                log::info!("[agent-model/completion] tab={} request={} phase=staged_claim refused=true at_ms={}", request.tab_id, request.id, crate::service::pty_frame_timing::unix_ms());
                return true;
            }
        }
        log::info!(
            "[agent-model/completion] tab={} request={} phase=staged_execute at_ms={}",
            request.tab_id,
            request.id,
            crate::service::pty_frame_timing::unix_ms()
        );
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
        let retry = matches!(result, Err(ModelSwitchError::InputNotReady(_)))
            && request.require_current(self).is_ok();
        let warn = {
            let mut mux = self.mux.write();
            let mut tracker = self.restart_tracker.lock();
            let warn = tracker.staged_models.warn_once(&request);
            if retry {
                tracker.staged_models.release(&request);
            } else {
                tracker.staged_models.finish(&request);
            }
            if let Some(tab) = mux.get_tab_mut(&request.tab_id) {
                finish_staged_selection(
                    &request,
                    tab,
                    &tracker.staged_models,
                    result.as_ref().ok(),
                );
            }
            warn
        };
        if let Err(error) = result {
            log::warn!("[agent-model/completion] tab={} request={} phase=staged_refused retry={} error={error}", request.tab_id, request.id, retry);
            if warn {
                self.create_notification_ext(
                    "Model switch pending".into(),
                    Some(format!("Tab {}: {error}", request.tab_id)),
                    Some("warning"),
                    None,
                    Some(false),
                    Default::default(),
                    Default::default(),
                    None,
                );
            }
        }
        self.emit_state_changed();
        !retry
    }
}

struct StagedFrames {
    svc: ServiceContext,
    pty_id: String,
    signal: std::sync::Arc<crate::pty::screen_state::ScreenChangeSignal>,
}

impl StagedFrames {
    fn new(svc: &ServiceContext, pty_id: &str) -> Self {
        let signal = svc.screen_state.lock().subscribe_screen_changes(pty_id);
        Self {
            svc: svc.clone(),
            pty_id: pty_id.into(),
            signal,
        }
    }
}

impl Drop for StagedFrames {
    fn drop(&mut self) {
        self.svc
            .screen_state
            .lock()
            .unsubscribe_screen_changes(&self.pty_id, &self.signal);
    }
}

fn finish_staged_selection(
    request: &SwitchRequest,
    tab: &mut Tab,
    pending: &PendingSwitches,
    outcome: Option<&ModelSwitchOutcome>,
) {
    let confirmed = outcome.is_some_and(|outcome| {
        outcome.tab_id == request.tab_id
            && outcome.session_id == request.session_id
            && outcome.requested_model == request.model
            && (matches!(&outcome.confirmation, ModelSwitchConfirmation::Observed { model, .. } if model == &request.model)
                || (outcome.confirmation == ModelSwitchConfirmation::Pending
                    && tab.pending_model_switch_observation.as_ref().is_some_and(|observation| {
                        observation.session_id == request.session_id && observation.launched_model == request.model
                    })))
    });
    let own_selection = tab
        .pending_model_selection
        .as_ref()
        .is_some_and(|selection| {
            selection.model == request.model && selection.effort == request.effort
        });
    let own_binding = tab.agent_session_id.as_deref() == Some(&request.session_id)
        && tab.session_binding_nonce.as_deref() == Some(&request.nonce)
        && tab.reported_session_binding_nonce.as_deref() == Some(&request.nonce);
    if confirmed && own_selection && own_binding && !pending.0.contains_key(&request.tab_id) {
        tab.pending_model_selection = None;
    }
}

fn completion_anchor(
    svc: &ServiceContext,
    tab_id: &str,
    session_id: &str,
) -> Option<crate::service::chat_send_transaction::SendEvidenceAnchor> {
    let (provider_id, cwd) = {
        let mux = svc.mux.read();
        let tab = mux.get_tab(tab_id)?;
        if tab.agent_session_id.as_deref() != Some(session_id) {
            return None;
        }
        (tab.agent_type.clone()?, tab.cwd.clone()?)
    };
    let (path, turn_normalization) =
        svc.agent_registry.with_provider(&provider_id, |provider| {
            (
                provider.session_jsonl_path(&cwd, session_id),
                provider.turn_evidence_normalization(),
            )
        })?;
    let offset = std::fs::metadata(path?).ok()?.len();
    Some(crate::service::chat_send_transaction::SendEvidenceAnchor {
        provider_id,
        cwd,
        session_id: session_id.into(),
        turn_normalization,
        window: crate::service::chat_send_transaction::EvidenceWindow::Jsonl {
            transcript_offset: offset,
        },
    })
}

fn staged_tab_idle(svc: &ServiceContext, tab_id: &str, deadline: Instant) -> bool {
    let started = Instant::now();
    let idle = crate::service::status_reconciliation::snapshot_with_reconciled_status_until(
        &svc.mux,
        &svc.agent_tracker,
        &svc.agent_registry,
        deadline,
    )
    .is_some_and(|snapshot| {
        snapshot.tab_statuses.get(tab_id) == Some(&codeterm_types::pty::TabStatus::Idle)
    });
    log::info!("[agent-model/completion] tab={tab_id} phase=staged_idle_gate idle={idle} elapsed_ms={} at_ms={}", started.elapsed().as_millis(), crate::service::pty_frame_timing::unix_ms());
    idle
}

#[cfg(test)]
mod tests;
