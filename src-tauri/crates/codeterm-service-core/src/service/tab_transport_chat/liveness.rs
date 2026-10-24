use super::*;
use std::sync::mpsc;
use std::time::Duration;

const TURN_START_DEADLINE: Duration = Duration::from_secs(60);

pub(crate) struct BoundSession {
    tab_id: String,
    pub(super) provider: Arc<dyn ChatBackendProvider>,
    state: Mutex<Health>,
}

#[derive(Default)]
struct Health {
    failure: Option<String>,
    turn_start: Option<mpsc::Sender<()>>,
    assistant_ids: std::collections::HashSet<String>,
}

impl BoundSession {
    pub(super) fn new(tab_id: String, provider: Arc<dyn ChatBackendProvider>) -> Self {
        Self {
            tab_id,
            provider,
            state: Mutex::new(Health::default()),
        }
    }

    pub(super) fn current(&self) -> Result<(), String> {
        if let Some(reason) = &self.state.lock().failure {
            return Err(reason.clone());
        }
        Ok(())
    }

    pub(super) fn preflight(&self, session: &str) -> Result<(), String> {
        self.current()?;
        let snapshot = self
            .provider
            .poll(session, None)
            .map_err(|error| error.to_string())?;
        let mut state = self.state.lock();
        if state.turn_start.is_none() {
            state.assistant_ids = snapshot
                .messages
                .into_iter()
                .filter(|message| message.kind == "assistant")
                .map(|message| message.id)
                .collect();
        }
        Ok(())
    }

    fn arm(&self) -> Option<mpsc::Receiver<()>> {
        let mut state = self.state.lock();
        if state.failure.is_some() || state.turn_start.is_some() {
            return None;
        }
        let (sender, receiver) = mpsc::channel();
        state.turn_start = Some(sender);
        Some(receiver)
    }

    pub(super) fn observe(&self, delta: &StructuredChat) {
        let mut state = self.state.lock();
        let new_assistant = delta.messages.iter().any(|message| {
            message.kind == "assistant" && !state.assistant_ids.contains(&message.id)
        });
        if delta.done == Some(false) || new_assistant {
            if let Some(sender) = state.turn_start.take() {
                let _ = sender.send(());
            }
        }
    }

    fn fail(&self, reason: &str) -> bool {
        let mut state = self.state.lock();
        if state.failure.is_some() {
            return false;
        }
        state.failure = Some(reason.into());
        state.turn_start.take();
        true
    }

    pub(super) fn cancel(&self) {
        self.state.lock().turn_start.take();
    }
}

impl ServiceContext {
    pub(crate) fn chatbackend_bound_session(
        &self,
        plugin: &str,
        session: &str,
    ) -> Result<Arc<BoundSession>, String> {
        self.ext::<Arc<TabReopenGuards>>().sessions.lock().get(&(plugin.into(), session.into())).cloned()
            .ok_or_else(|| "tab_busy_refused: chat_backend_unavailable: no live session binding; restart the tab".into())
    }

    pub(crate) fn chatbackend_liveness_failure(
        &self,
        plugin: &str,
        session: &str,
        bound: &BoundSession,
        reason: &str,
    ) {
        let mut mux = self.mux.write();
        let Some(tab) = mux.get_tab_mut(&bound.tab_id) else {
            return;
        };
        if !matches!(&tab.backing, crate::multiplexer::types::TabBacking::ChatBackend { plugin_id, session_id } if plugin_id == plugin && session_id == session)
        {
            return;
        }
        if !bound.fail(reason) {
            return;
        }
        let recovery = format!("domios agent restart --tab {}", bound.tab_id);
        let message = format!("Chat backend for tab {} is unavailable: {reason}. Recover with `{recovery}`. Accepted input will not be replayed.", bound.tab_id);
        tab.boot_stage = Some(codeterm_types::pty::BootStage::SpawnFailed);
        tab.boot_failure_reason = Some(message.clone());
        let mut live = codeterm_types::pty::AgentLiveState::from(codeterm_types::pty::TabVerdict {
            state: codeterm_types::pty::TimelineState::Question,
            subtype: Some(codeterm_types::pty::QuestionSubtype::Error),
        });
        live.status_line = Some(message);
        tab.agent_state = Some(live);
        let nonce = tab.session_binding_nonce.clone();
        drop(mux);
        self.emit_state_changed(super::super::MuxEmitCause::TabLifecycle);
        if let Some(nonce) = nonce {
            self.recover_dead_chatbackend(&bound.tab_id, &nonce, 0, reason);
        }
    }

    pub(crate) fn arm_chatbackend_turn_deadline(&self, bound: &Arc<BoundSession>) {
        let Some(receiver) = bound.arm() else {
            return;
        };
        let Some(nonce) = self
            .mux
            .read()
            .get_tab(&bound.tab_id)
            .and_then(|tab| tab.session_binding_nonce.clone())
        else {
            bound.cancel();
            return;
        };
        let svc = self.clone();
        let bound = bound.clone();
        std::thread::spawn(move || {
            if receiver.recv_timeout(TURN_START_DEADLINE) == Err(mpsc::RecvTimeoutError::Timeout) {
                bound.cancel();
                svc.chatbackend_turn_start_incident(&bound.tab_id, &nonce, 0);
            }
        });
    }
}

#[cfg(test)]
mod tests;
