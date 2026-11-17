use std::sync::mpsc::Sender;
use std::time::Instant;

use super::{ModelSwitchError, TabRestartSnapshot};
use crate::agent::provider::ModelSwitchMode;
use crate::multiplexer::types::{RestorableLaunchParameters, Tab};
use crate::service::ServiceContext;

pub struct StagedSwitch {
    pub request: SwitchRequest,
    pub idle: Sender<()>,
    pub claimed: bool,
    pub warned: bool,
}

pub struct LatchedCompletion {
    pub generation: u64,
    pub nonce: String,
    pub anchor: crate::service::chat_send_transaction::SendEvidenceAnchor,
}

#[derive(Default)]
pub struct PendingSwitches(
    pub std::collections::HashMap<String, StagedSwitch>,
    pub  std::collections::HashMap<
        String,
        std::sync::Weak<crate::pty::screen_state::ScreenChangeSignal>,
    >,
    pub std::collections::HashMap<String, (String, String, u64)>,
    pub std::collections::HashMap<String, LatchedCompletion>,
    pub std::collections::HashMap<String, u64>,
);

impl PendingSwitches {
    pub fn completion_generation(&self, tab: &str, session: &str, nonce: &str) -> u64 {
        self.2
            .get(tab)
            .filter(|(own_session, own_nonce, _)| own_session == session && own_nonce == nonce)
            .map_or(0, |(_, _, generation)| *generation)
    }

    pub fn completed(&self, request: &SwitchRequest) -> bool {
        let request = self
            .0
            .get(&request.tab_id)
            .filter(|staged| staged.request.id == request.id)
            .map_or(request, |staged| &staged.request);
        self.completion_generation(&request.tab_id, &request.session_id, &request.nonce)
            > request.completion_generation
    }

    pub fn signal_started(&mut self, tab_id: &str) {
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

    pub fn signal_completed(&mut self, tab: &str, session: &str, nonce: &str) {
        let generation = self
            .completion_generation(tab, session, nonce)
            .saturating_add(1);
        self.2
            .insert(tab.into(), (session.into(), nonce.into(), generation));
        self.signal_idle(tab);
    }
    pub fn boundary_generation(
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

    pub fn start_epoch(&self, tab: &str) -> u64 {
        self.4.get(tab).copied().unwrap_or_default()
    }

    pub fn latch_completed(
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

    pub fn replace(&mut self, mut request: SwitchRequest, idle: Sender<()>) {
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

    pub fn replace_at_idle(
        &mut self,
        request: SwitchRequest,
        idle: Sender<()>,
        already_idle: bool,
    ) {
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

    pub fn cancel(&mut self, tab_id: &str) {
        self.signal_idle(tab_id);
        if let Some(previous) = self.0.remove(tab_id) {
            let _ = previous.idle.send(());
        }
    }

    pub fn signal_idle(&self, tab_id: &str) {
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

    pub fn observe_command_idle(
        &mut self,
        tab_id: &str,
        signal: &std::sync::Arc<crate::pty::screen_state::ScreenChangeSignal>,
    ) {
        self.1.retain(|_, signal| signal.strong_count() > 0);
        self.1
            .insert(tab_id.into(), std::sync::Arc::downgrade(signal));
    }

    pub fn claim(&mut self, request: &SwitchRequest) -> bool {
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

    pub fn finish(&mut self, request: &SwitchRequest) {
        if self
            .0
            .get(&request.tab_id)
            .is_some_and(|staged| staged.request.id == request.id && staged.claimed)
        {
            self.0.remove(&request.tab_id);
        }
    }

    pub fn release(&mut self, request: &SwitchRequest) -> bool {
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

    pub fn warn_once(&mut self, request: &SwitchRequest) -> bool {
        self.0
            .get_mut(&request.tab_id)
            .filter(|staged| staged.request.id == request.id)
            .is_none_or(|staged| !std::mem::replace(&mut staged.warned, true))
    }
}

#[derive(Clone)]
pub struct SwitchRequest {
    pub id: String,
    pub tab_id: String,
    pub provider: String,
    pub session_id: String,
    pub pty_id: String,
    pub nonce: String,
    pub launch: RestorableLaunchParameters,
    pub model: String,
    pub effort: Option<String>,
    pub force: bool,
    pub restart_fallback: bool,
    pub mechanism: ModelSwitchMode,
    pub deadline: Instant,
    pub completion_generation: u64,
    pub completion_epoch: u64,
}

impl SwitchRequest {
    pub fn require_mechanism(&self, mechanism: ModelSwitchMode) -> Result<(), ModelSwitchError> {
        if self.mechanism == mechanism {
            Ok(())
        } else {
            Err(ModelSwitchError::NotSwitchable(
                "staged model switch refused: provider switch capability changed".into(),
            ))
        }
    }
    pub fn matches(&self, tab: &Tab, now: Instant) -> bool {
        now < self.deadline
            && tab.agent_type.as_deref() == Some(&self.provider)
            && tab.agent_session_id.as_deref() == Some(&self.session_id)
            && tab.pty_id.as_deref() == Some(&self.pty_id)
            && tab.session_binding_nonce.as_deref() == Some(&self.nonce)
            && tab.reported_session_binding_nonce.as_deref() == Some(&self.nonce)
            && tab.restorable_launch == self.launch
    }

    pub fn require_current(&self, svc: &ServiceContext) -> Result<(), ModelSwitchError> {
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

    pub fn require_snapshot(&self, snap: &TabRestartSnapshot) -> Result<(), ModelSwitchError> {
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
