use super::*;
use std::path::PathBuf;
use std::sync::Arc;

pub(super) const PENDING_DETAIL: &str = "Switch sent; confirms when this turn ends";

pub(super) struct CommandEvidence {
    pub path: PathBuf,
    pub boundary: u64,
    pub pattern: Regex,
}

pub(super) fn can_await_buffered_stdout(
    buffering: bool,
    changed: bool,
    picker_submitted: bool,
    prompt_answered: bool,
    normal_input: bool,
) -> bool {
    buffering && changed && picker_submitted && prompt_answered && normal_input
}

struct Observer {
    svc: ServiceContext,
    target: CommandTarget,
    id: String,
    signal: Arc<crate::pty::screen_state::ScreenChangeSignal>,
    settled: bool,
}

impl Observer {
    fn current(&self) -> bool {
        self.target.current(&self.svc)
            && self
                .svc
                .ext::<crate::service::agent_restart::RestartTracker>()
                .lock()
                .pending_command_observers
                .get(&self.target.tab_id)
                .is_some_and(|(id, _)| id == &self.id)
    }
}

impl Drop for Observer {
    fn drop(&mut self) {
        let own = {
            let mut tracker = self
                .svc
                .ext::<crate::service::agent_restart::RestartTracker>()
                .lock();
            if tracker
                .pending_command_observers
                .get(&self.target.tab_id)
                .is_some_and(|(id, _)| id == &self.id)
            {
                tracker
                    .pending_command_observers
                    .remove(&self.target.tab_id);
                true
            } else {
                false
            }
        };
        self.svc
            .screen_state
            .unsubscribe_screen_changes(&self.target.pty_id, &self.signal);
        if own && self.settled {
            if let Some(tab) = self.svc.mux.write().get_tab_mut(&self.target.tab_id) {
                if tab
                    .pending_model_selection
                    .as_ref()
                    .is_some_and(|selection| selection.confirmation_pending)
                {
                    tab.pending_model_selection = None;
                }
            }
            self.svc.emit_state_changed(MuxEmitCause::Model);
        }
    }
}

impl ServiceContext {
    pub(super) fn observe_pending_model_commands(
        &self,
        snap: TabRestartSnapshot,
        target: CommandTarget,
        evidence: Vec<CommandEvidence>,
        source: ModelSwitchTranscript,
        effort: Option<String>,
        model_changed: bool,
    ) {
        let signal = command_signal(&self.screen_state, &target.pty_id);
        crate::service::boot_wait::observe_boot_changes(&signal);
        let id = uuid::Uuid::new_v4().to_string();
        {
            let mut mux = self.mux.write();
            let Some(tab) = mux
                .get_tab_mut(&target.tab_id)
                .filter(|tab| target.matches(tab))
            else {
                return;
            };
            tab.pending_model_selection = Some(crate::multiplexer::types::PendingModelSelection {
                model: target.model.clone(),
                effort: effort.clone(),
                confirmation_pending: true,
            });
            let previous = self
                .ext::<crate::service::agent_restart::RestartTracker>()
                .lock()
                .pending_command_observers
                .insert(target.tab_id.clone(), (id.clone(), signal.clone()));
            if let Some((_, previous)) = previous {
                previous.mark_changed();
            }
        }
        self.emit_state_changed(MuxEmitCause::Model);
        let observer = Observer {
            svc: self.clone(),
            target,
            id,
            signal,
            settled: false,
        };
        let wake = observer.signal.clone();
        let mut watcher =
            match notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if event.is_ok_and(|event| {
                    matches!(
                        event.kind,
                        notify::EventKind::Modify(_) | notify::EventKind::Create(_)
                    )
                }) {
                    wake.mark_changed();
                }
            }) {
                Ok(watcher) => watcher,
                Err(error) => {
                    log::error!("[agent-model] pending confirmation observer unavailable: {error}");
                    return;
                }
            };
        for path in evidence
            .iter()
            .map(|entry| &entry.path)
            .collect::<std::collections::HashSet<_>>()
        {
            if let Err(error) =
                notify::Watcher::watch(&mut watcher, path, notify::RecursiveMode::NonRecursive)
            {
                log::error!("[agent-model] pending confirmation watch unavailable: {error}");
                return;
            }
        }
        if let Err(error) = std::thread::Builder::new()
            .name("pending-model-command".into())
            .spawn(move || {
                let mut observer = observer;
                let _watcher = watcher;
                let mut unknown_reported = false;
                loop {
                    let generation = observer.signal.generation();
                    if !observer.current() {
                        observer.settled = true;
                        break;
                    }
                    if !unknown_reported {
                        let screen = observer.svc.screen_state.get_screen_text(&observer.target.pty_id).unwrap_or_default();
                        let unknown = observer.target.readiness.as_ref().is_some_and(|ready| ready.has_dialog(&screen))
                            && observer.svc.agent_registry.with_provider(&observer.target.provider, |provider| provider.parse_prompt(&screen).is_none()).unwrap_or(false);
                        if unknown {
                            unknown_reported = true;
                                observer.svc.during_turn_switch_incident(&snap, ModelSwitchError::DuringTurnIncident {
                                    stage: "pending:UnknownPrompt".into(), screen,
                                    reason: "an unrecognized confirmation is waiting; the sent switch remains unconfirmed".into(),
                                });
                        }
                    }
                    if evidence.iter().all(|entry| {
                        read_confirmation(&entry.path, entry.boundary, &source, &entry.pattern)
                    }) {
                        if !observer.current() {
                            break;
                        }
                        let target = &observer.target;
                        {
                            let mut mux = observer.svc.mux.write();
                            let Some(tab) = mux
                                .get_tab_mut(&target.tab_id)
                                .filter(|tab| target.matches(tab))
                            else {
                                break;
                            };
                            let tracker = observer
                                .svc
                                .ext::<crate::service::agent_restart::RestartTracker>()
                                .lock();
                            if !tracker
                                .pending_command_observers
                                .get(&target.tab_id)
                                .is_some_and(|(id, _)| id == &observer.id)
                            {
                                break;
                            }
                            record_live_request_identity(tab, &target.model, effort.as_deref());
                            if model_changed {
                                tab.agent_session_model = Some(target.model.clone());
                                tab.agent_session_model_identity =
                                    Some(crate::multiplexer::types::SessionModelIdentity {
                                        session_id: target.session_id.clone(),
                                        session_binding_nonce: target.binding_nonce.clone(),
                                        launch_report: None,
                                        model_switch_turn_confirmed: true,
                                    });
                                tab.model_identity_mismatch = None;
                                if let Some(fallback) = tab.model_fallback.as_mut() { fallback.actual_model = Some(target.model.clone()); }
                            }
                            if let Some(effort) = effort.as_deref() {
                                record_confirmed_effort(tab, effort);
                            }
                        }
                        observer
                            .svc
                            .persist_current_attempt_session_model(&target.tab_id);
                        let _ = crate::service::session_persist::persist_session_after_mutation(
                            &observer.svc,
                            crate::service::session_persist::SessionPersistUrgency::Prompt,
                        );
                        observer.settled = true;
                        break;
                    }
                    if !observer.signal.wait_until_changed(generation) {
                        observer.settled = true;
                        break;
                    }
                }
            })
        {
            log::error!("[agent-model] pending confirmation worker unavailable: {error}");
        }
    }
}

#[cfg(test)]
mod tests;
