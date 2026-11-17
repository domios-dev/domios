use crate::service::MuxEmitCause;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use super::*;
use crate::multiplexer::types::{PendingModelSelection, Tab};
pub(super) use crate::service::agent_lifecycle::staged_switches::{PendingSwitches, SwitchRequest};

const STAGING_DEADLINE: Duration = Duration::from_secs(3600);
pub mod completion_records;

pub struct StagingParams {
    pub mechanism: ModelSwitchMode,
    pub force: bool,
    pub restart_fallback: bool,
    pub previous_model: Option<String>,
    pub completion_boundary: CompletionBoundary,
}

pub struct CompletionBoundary {
    session_id: String,
    nonce: String,
    generation: u64,
    epoch: u64,
    records: Option<completion_records::RecordBoundary>,
}

pub trait StagedSwitchExt {
    fn staged_completion_boundary(&self, tab_id: &str) -> Option<CompletionBoundary>;
    fn cancel_staged_model_switch(&self, tab_id: &str);
    fn stage_model_switch(
        &self,
        snap: &TabRestartSnapshot,
        session_id: &str,
        model: &str,
        effort: Option<&str>,
        params: StagingParams,
    ) -> Result<ModelSwitchOutcome, ModelSwitchError>;
    fn settle_staged_model_switch(&self, request: SwitchRequest, idle: bool) -> bool;
}

impl StagedSwitchExt for ServiceContext {
    fn staged_completion_boundary(&self, tab_id: &str) -> Option<CompletionBoundary> {
        let (session_id, nonce, mut generation, epoch, provider, cwd, completed_anchor, working) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id)?;
            let session_id = tab.agent_session_id.clone()?;
            let nonce = tab.session_binding_nonce.clone()?;
            if tab.reported_session_binding_nonce.as_ref() != Some(&nonce) {
                return None;
            }
            let tracker = self
                .ext::<crate::service::agent_restart::RestartTracker>()
                .lock();
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
                self.resolved_tab_status(tab) == Some(codeterm_types::pty::TabStatus::Working),
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
            let current_generation = generation;
            generation = {
                let mux = self.mux.read();
                let still_not_working = mux.get_tab(tab_id).is_some_and(|tab| {
                    self.resolved_tab_status(tab) != Some(codeterm_types::pty::TabStatus::Working)
                });
                let reuse = completion_reuse_allowed(
                    working || !still_not_working,
                    records
                        .as_ref()
                        .is_some_and(|records| records.in_progress()),
                    no_foreign_turn,
                );
                if !reuse {
                    log::info!("[agent-model/completion] tab={tab_id} phase=accepted_completion_reuse_refused working={} declared_turn_in_progress={} foreign_turn={}", working || !still_not_working, records.as_ref().is_some_and(|records| records.in_progress()), !no_foreign_turn);
                }
                self.ext::<crate::service::agent_restart::RestartTracker>()
                    .lock()
                    .staged_models
                    .boundary_generation(tab_id, &session_id, &nonce, reuse)
            };
            if generation < current_generation {
                log::info!(
                    "[agent-model/completion] tab={tab_id} phase=accepted_completion_latched"
                );
            }
        }
        Some(CompletionBoundary {
            session_id,
            nonce,
            generation,
            epoch,
            records,
        })
    }

    fn cancel_staged_model_switch(&self, tab_id: &str) {
        let mut mux = self.mux.write();
        self.ext::<crate::service::agent_restart::RestartTracker>()
            .lock()
            .staged_models
            .cancel(tab_id);
        if let Some(tab) = mux.get_tab_mut(tab_id) {
            if !tab
                .pending_model_selection
                .as_ref()
                .is_some_and(|selection| selection.confirmation_pending)
            {
                tab.pending_model_selection = None;
            }
        }
        drop(mux);
        self.emit_state_changed(MuxEmitCause::Model);
    }

    fn stage_model_switch(
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
                confirmation_pending: false,
            });
            let already_idle =
                self.resolved_tab_status(tab) == Some(codeterm_types::pty::TabStatus::Idle);
            self.ext::<crate::service::agent_restart::RestartTracker>()
                .lock()
                .staged_models
                .replace_at_idle(request.clone(), tx, already_idle);
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
                        .ext::<crate::service::agent_restart::RestartTracker>()
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
                        .ext::<crate::service::agent_restart::RestartTracker>()
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
                        let mut tracker = svc.ext::<crate::service::agent_restart::RestartTracker>().lock();
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
        self.emit_state_changed(MuxEmitCause::Model);
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
            control_changes: Vec::new(),
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
            let mut tracker = self
                .ext::<crate::service::agent_restart::RestartTracker>()
                .lock();
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
            let mut tracker = self
                .ext::<crate::service::agent_restart::RestartTracker>()
                .lock();
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
                self.log_system_notice(
                    "Model switch pending".into(),
                    Some(format!("Tab {}: {error}", request.tab_id)),
                    Some("warning"),
                );
            }
        }
        self.emit_state_changed(MuxEmitCause::Model);
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
        let signal = svc.screen_state.subscribe_screen_changes(pty_id);
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

fn completion_reuse_allowed(
    working: bool,
    declared_turn_in_progress: bool,
    no_foreign_turn: bool,
) -> bool {
    !working && !declared_turn_in_progress && no_foreign_turn
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
