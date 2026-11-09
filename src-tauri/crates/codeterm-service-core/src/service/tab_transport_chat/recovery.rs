use super::*;
use std::time::{Duration, Instant};

const COOLDOWN: Duration = Duration::from_secs(300);

#[derive(Default)]
pub(super) struct Recovery {
    attempted_epoch: Option<String>,
    incident_epoch: Option<String>,
    liveness_incident_epoch: Option<String>,
    last_attempt: Option<Instant>,
    in_flight: bool,
    exit_pending: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum Decision {
    Restart,
    Incident,
    Ignore,
}

impl Recovery {
    fn observe_stall(&mut self, epoch: &str) -> bool {
        if self.liveness_incident_epoch.as_deref() == Some(epoch)
            || self.in_flight
            || self.incident_epoch.as_deref() == Some(epoch)
        {
            return false;
        }
        self.liveness_incident_epoch = Some(epoch.into());
        true
    }

    fn claim(&mut self, epoch: &str, now: Instant) -> Decision {
        if self.attempted_epoch.as_deref() == Some(epoch)
            || self.incident_epoch.as_deref() == Some(epoch)
        {
            return Decision::Ignore;
        }
        self.attempted_epoch = Some(epoch.into());
        if self.in_flight
            || self
                .last_attempt
                .is_some_and(|last| now.saturating_duration_since(last) < COOLDOWN)
        {
            self.incident_epoch = Some(epoch.into());
            return Decision::Incident;
        }
        self.last_attempt = Some(now);
        self.in_flight = true;
        Decision::Restart
    }
}

struct RecoveryCompletion<'a> {
    service: &'a ServiceContext,
    tab_id: &'a str,
    owned: bool,
}

impl Drop for RecoveryCompletion<'_> {
    fn drop(&mut self) {
        if self.owned {
            if let Some(recovery) = self
                .service
                .ext::<Arc<TabReopenGuards>>()
                .recoveries
                .lock()
                .get_mut(self.tab_id)
            {
                recovery.in_flight = false;
            }
        }
        self.service.wake_user_owner_on_state_change(self.tab_id);
    }
}

/// Held sends wait while a dead runtime's exit is handled, because recovery may take over the relaunch.
pub(crate) struct RuntimeExitHandling {
    service: ServiceContext,
    tab_id: String,
}

impl Drop for RuntimeExitHandling {
    fn drop(&mut self) {
        if let Some(recovery) = self
            .service
            .ext::<Arc<TabReopenGuards>>()
            .recoveries
            .lock()
            .get_mut(&self.tab_id)
        {
            recovery.exit_pending = false;
        }
        self.service.wake_user_owner_on_state_change(&self.tab_id);
    }
}

impl ServiceContext {
    pub(crate) fn begin_runtime_exit(&self, tab_id: &str) -> RuntimeExitHandling {
        self.ext::<Arc<TabReopenGuards>>()
            .recoveries
            .lock()
            .entry(tab_id.into())
            .or_default()
            .exit_pending = true;
        RuntimeExitHandling {
            service: self.clone(),
            tab_id: tab_id.into(),
        }
    }

    /// True until automatic recovery knows its outcome, so a transient degrade settles no held send.
    pub(crate) fn runtime_recovery_owns_sends(&self, tab_id: &str) -> bool {
        self.try_ext::<Arc<TabReopenGuards>>()
            .is_some_and(|guards| {
                guards
                    .recoveries
                    .lock()
                    .get(tab_id)
                    .is_some_and(|recovery| recovery.in_flight || recovery.exit_pending)
            })
    }

    pub(crate) fn record_runtime_interruption_for_tab(
        &self,
        tab_id: &str,
        session: &str,
        nonce: &str,
        epoch: u64,
        reason: &str,
    ) -> Result<Option<(String, String, String)>, String> {
        let binding = self
            .mux
            .read()
            .get_tab(tab_id)
            .filter(|tab| {
                tab.session_binding_nonce.as_deref() == Some(nonce)
                    && tab.agent_session_id.as_deref() == Some(session)
            })
            .and_then(|tab| tab.task_binding.clone());
        let Some(binding) = binding else {
            return Ok(None);
        };
        let (Some(plan), Some(task), Some(attempt)) =
            (binding.plan_id, binding.task_id, binding.attempt_id)
        else {
            return Err("runtime interruption has an incomplete task binding".into());
        };
        self.plan_store
            .record_runtime_interruption(
                &plan,
                &task,
                &attempt,
                tab_id,
                session,
                nonce,
                epoch,
                reason,
                chrono::Utc::now().timestamp_millis().max(0) as u64,
            )
            .map_err(|error| error.to_string())?;
        Ok(Some((plan, task, attempt)))
    }

    pub(crate) fn chatbackend_turn_start_incident(&self, tab_id: &str, nonce: &str, epoch: u64) {
        let reason = "Accepted send had no observed turn start within 60 seconds";
        {
            let mut mux = self.mux.write();
            let Some(tab) = mux.get_tab_mut(tab_id).filter(|tab| {
                tab.is_chat_backend() && tab.session_binding_nonce.as_deref() == Some(nonce)
            }) else {
                return;
            };
            let mut live =
                codeterm_types::pty::AgentLiveState::from(codeterm_types::pty::TabVerdict {
                    state: codeterm_types::pty::TimelineState::Question,
                    subtype: Some(codeterm_types::pty::QuestionSubtype::Error),
                });
            live.status_line = Some(reason.into());
            tab.agent_state = Some(live);
        }
        let notify = self
            .ext::<Arc<TabReopenGuards>>()
            .recoveries
            .lock()
            .entry(tab_id.into())
            .or_default()
            .observe_stall(&format!("{nonce}:{epoch}"));
        self.emit_state_changed(super::super::MuxEmitCause::RuntimeSignal);
        if notify {
            self.chatbackend_incident(tab_id, reason);
        }
    }

    pub(crate) fn recover_dead_chatbackend(
        &self,
        tab_id: &str,
        nonce: &str,
        epoch: u64,
        reason: &str,
    ) {
        self.recover_interrupted_chatbackend(tab_id, nonce, epoch, reason, false);
    }

    pub(crate) fn recover_interrupted_chatbackend(
        &self,
        tab_id: &str,
        nonce: &str,
        epoch: u64,
        reason: &str,
        interrupted: bool,
    ) {
        let expected = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(tab_id).filter(|tab| {
                tab.is_chat_backend() && tab.session_binding_nonce.as_deref() == Some(nonce)
            }) else {
                return;
            };
            tab.agent_session_id.clone()
        };
        let Some(expected) = expected else {
            return;
        };
        if super::super::agent_runtime::engine::for_tab(tab_id)
            .is_some_and(|runtime| runtime.context.lock().epoch != epoch)
        {
            return;
        }
        let key = format!("{nonce}:{epoch}");
        let decision = self
            .ext::<Arc<TabReopenGuards>>()
            .recoveries
            .lock()
            .entry(tab_id.into())
            .or_default()
            .claim(&key, Instant::now());
        if decision == Decision::Ignore {
            return;
        }
        if let Some(runtime) = super::super::agent_runtime::engine::for_tab(tab_id) {
            if runtime.context.lock().binding_nonce == nonce {
                runtime.state.lock().degrade(
                    epoch,
                    &super::super::agent_runtime::model::RuntimeFailure::Refused(reason.into()),
                );
                super::super::agent_runtime::engine::publish(self, tab_id, &runtime);
            }
        }
        let mut live = codeterm_types::pty::AgentLiveState::from(codeterm_types::pty::TabVerdict {
            state: codeterm_types::pty::TimelineState::Question,
            subtype: Some(codeterm_types::pty::QuestionSubtype::Error),
        });
        live.status_line = Some(reason.into());
        crate::agent::state_verdict::apply_live(&self.mux, tab_id, &expected, live);
        self.emit_state_changed(super::super::MuxEmitCause::RuntimeSignal);
        let svc = self.clone();
        let tab_id = tab_id.to_owned();
        let nonce = nonce.to_owned();
        let reason = reason.to_owned();
        std::thread::spawn(move || {
            let _completion = RecoveryCompletion {
                service: &svc,
                tab_id: &tab_id,
                owned: decision == Decision::Restart,
            };
            if super::super::agent_runtime::engine::for_tab(&tab_id)
                .is_some_and(|runtime| runtime.context.lock().epoch != epoch)
            {
                return;
            }
            if !svc.mux.read().get_tab(&tab_id).is_some_and(|tab| {
                tab.session_binding_nonce.as_deref() == Some(&nonce)
                    && tab.agent_session_id.as_deref() == Some(&expected)
            }) {
                return;
            }
            let binding = match svc
                .record_runtime_interruption_for_tab(&tab_id, &expected, &nonce, epoch, &reason)
            {
                Ok(binding) => binding,
                Err(error) => {
                    svc.chatbackend_incident(
                        &tab_id,
                        &format!("{reason}. Cannot persist interruption: {error}"),
                    );
                    return;
                }
            };
            let failure = if decision == Decision::Restart {
                match svc.agent_restart(&tab_id, false, 0) {
                    Ok(result)
                        if result.agent_ready
                            && result.session_action() == "resumed"
                            && result.session_id.as_deref() == Some(&expected) =>
                    {
                        {
                            let mut mux = svc.mux.write();
                            if let Some(tab) = mux.get_tab_mut(&tab_id).filter(|tab| {
                                tab.agent_session_id.as_deref() == Some(&expected)
                                    && tab
                                        .agent_runtime_state
                                        .as_ref()
                                        .is_some_and(|state| state["state"] == "loaded")
                                    && tab.agent_state.as_ref().is_some_and(|state| {
                                        state.status_line.as_deref() == Some(&reason)
                                    })
                            }) {
                                tab.agent_state = Some(codeterm_types::pty::AgentLiveState::from(
                                    codeterm_types::pty::TabVerdict {
                                        state: codeterm_types::pty::TimelineState::Idle,
                                        subtype: None,
                                    },
                                ));
                            }
                        }
                        if let Some(runtime) = super::super::agent_runtime::engine::for_tab(&tab_id)
                        {
                            super::super::agent_runtime::engine::publish(&svc, &tab_id, &runtime);
                        }
                        if let Some((plan, task, attempt)) = &binding {
                            let new_nonce = svc
                                .mux
                                .read()
                                .get_tab(&tab_id)
                                .and_then(|tab| tab.session_binding_nonce.clone())
                                .unwrap_or_default();
                            if let Err(error) = svc.plan_store.record_runtime_recovery(
                                plan,
                                task,
                                attempt,
                                &tab_id,
                                &expected,
                                &new_nonce,
                                &nonce,
                                epoch,
                                chrono::Utc::now().timestamp_millis().max(0) as u64,
                            ) {
                                svc.chatbackend_incident(
                                    &tab_id,
                                    &format!(
                                        "Runtime resumed, but recovery receipt failed: {error}"
                                    ),
                                );
                                return;
                            }
                        }
                        svc.emit_state_changed(super::super::MuxEmitCause::RuntimeSignal);
                        if interrupted {
                            svc.continue_recovered_runtime(&tab_id, &expected, &key);
                        }
                        return;
                    }
                    Ok(_) => {
                        "Automatic restart did not confirm the same resumed session".to_string()
                    }
                    Err(error) => format!("Automatic restart failed: {error:?}"),
                }
            } else {
                "Automatic restart cooldown is active".into()
            };
            if let Some(runtime) = super::super::agent_runtime::engine::for_tab(&tab_id) {
                let context = runtime.context.lock().clone();
                if let Some(recovery) = svc
                    .ext::<Arc<TabReopenGuards>>()
                    .recoveries
                    .lock()
                    .get_mut(&tab_id)
                {
                    recovery.incident_epoch =
                        Some(format!("{}:{}", context.binding_nonce, context.epoch));
                }
            }
            svc.chatbackend_incident(&tab_id, &format!("{reason}. {failure}"));
        });
    }

    pub(crate) fn chatbackend_incident(&self, tab_id: &str, reason: &str) {
        self.create_notification_actions_reported(
            "Chat backend unavailable".into(),
            Some(format!("Tab {tab_id}: {reason}. Recover with `domios agent restart --tab {tab_id}`. Accepted input will not be replayed.")),
            Some("warning"), crate::notification::store::Ask::default(), Some(false),
            crate::service::notification::NotificationOrigin::local_tab(tab_id.into()),
            crate::service::notification::PushIntent {
                level: Some(crate::notification::push::NotifyLevel::ActionNeeded),
                event_key: Some(uuid::Uuid::new_v4().to_string()),
            }, None,
        );
    }
}

#[cfg(test)]
mod tests;
