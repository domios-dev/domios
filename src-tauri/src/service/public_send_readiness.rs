use crate::multiplexer::types::Tab;
use crate::service::launch_readiness::LaunchAttempt;
use crate::service::ServiceContext;
use codeterm_types::pty::BootStage;
use parking_lot::{Condvar, Mutex};
use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

#[cfg(test)]
#[path = "public_send_readiness/tests.rs"]
mod tests;

#[derive(Clone)]
struct LaunchSendGate {
    attempt: LaunchAttempt,
    startup_turn_required: bool,
    startup_turn_session: Option<String>,
}

#[derive(Default)]
struct ReadinessState {
    gates: HashMap<String, LaunchSendGate>,
    generation: u64,
}

#[derive(Default)]
struct ReadinessRegistry {
    state: Mutex<ReadinessState>,
    changed: Condvar,
}

static READINESS: LazyLock<ReadinessRegistry> = LazyLock::new(ReadinessRegistry::default);

fn launch_gate_ready(tab: &Tab, gate: &LaunchSendGate) -> Result<bool, String> {
    if tab.pty_id.as_deref() != Some(gate.attempt.pty_id.as_str())
        || tab.session_binding_nonce.as_deref() != Some(gate.attempt.binding_nonce.as_str())
    {
        return Err("launch attempt changed before public input readiness".into());
    }
    if tab.boot_stage == Some(BootStage::SpawnFailed) {
        return Err(tab
            .boot_failure_reason
            .clone()
            .unwrap_or_else(|| "agent launch failed".into()));
    }
    let session_bound = tab.launch_session_authenticated();
    let startup_turn_observed = !gate.startup_turn_required
        || gate.startup_turn_session.as_deref() == tab.agent_session_id.as_deref();
    Ok(session_bound && startup_turn_observed)
}

fn ready_without_launch_gate(tab: &Tab) -> Option<Result<(), String>> {
    if tab.agent_type.is_none() || tab.boot_stage == Some(BootStage::PtyReattached) {
        return Some(Ok(()));
    }
    if tab.boot_stage == Some(BootStage::SpawnFailed) {
        return Some(Err(tab
            .boot_failure_reason
            .clone()
            .unwrap_or_else(|| "agent launch failed".into())));
    }
    if tab.agent_booting {
        return None;
    }
    match tab.boot_stage {
        Some(
            BootStage::PtyStarting
            | BootStage::PtySpawned
            | BootStage::ReaderReady
            | BootStage::ShellReady
            | BootStage::AgentLaunching
            | BootStage::LaunchWritten
            | BootStage::ProviderReady
            | BootStage::PreparingRoleInstructions
            | BootStage::DeliveringInstructions,
        ) => None,
        Some(BootStage::AgentReady) | None if tab.launch_session_authenticated() => Some(Ok(())),
        Some(BootStage::AgentReady) | None => None,
        Some(BootStage::LaunchDeadlineElapsed | BootStage::LaunchBindingUnconfirmed) => None,
        Some(BootStage::SpawnFailed | BootStage::PtyReattached) => unreachable!(),
    }
}

impl ReadinessRegistry {
    fn arm(&self, attempt: LaunchAttempt, startup_turn_required: bool) {
        let mut state = self.state.lock();
        state.gates.insert(
            attempt.tab_id.clone(),
            LaunchSendGate {
                attempt,
                startup_turn_required,
                startup_turn_session: None,
            },
        );
        state.generation = state.generation.wrapping_add(1);
        self.changed.notify_all();
    }

    fn notify(&self) {
        let mut state = self.state.lock();
        state.generation = state.generation.wrapping_add(1);
        self.changed.notify_all();
    }

    fn retire(&self, tab_id: &str) {
        let mut state = self.state.lock();
        if state.gates.remove(tab_id).is_some() {
            state.generation = state.generation.wrapping_add(1);
            self.changed.notify_all();
        }
    }

    fn note_turn(
        &self,
        mux: &crate::MuxState,
        tab_id: &str,
        session_id: &str,
        binding_nonce: &str,
        messages: &[crate::agent::chat_types::ChatMsg],
    ) -> bool {
        if !messages.iter().any(|message| message.kind == "user") {
            return false;
        }
        let tab_identity = {
            let mux = mux.read();
            let Some(tab) = mux.get_tab(tab_id) else {
                return false;
            };
            if !tab.launch_session_authenticated()
                || tab.agent_session_id.as_deref() != Some(session_id)
                || tab.session_binding_nonce.as_deref() != Some(binding_nonce)
                || tab.reported_session_binding_nonce.as_deref() != Some(binding_nonce)
            {
                return false;
            }
            (tab.pty_id.clone(), tab.session_binding_nonce.clone())
        };
        let mut state = self.state.lock();
        let Some(gate) = state.gates.get_mut(tab_id) else {
            return false;
        };
        if !gate.startup_turn_required
            || gate.attempt.pty_id != tab_identity.0.as_deref().unwrap_or_default()
            || gate.attempt.binding_nonce != binding_nonce
        {
            return false;
        }
        gate.startup_turn_session = Some(session_id.to_string());
        state.generation = state.generation.wrapping_add(1);
        self.changed.notify_all();
        log::info!(
            "[public-send-readiness] event=LaunchBootstrapTurnConfirmed tab={tab_id} session={session_id}"
        );
        true
    }

    fn inspect(&self, mux: &crate::MuxState, tab_id: &str) -> Option<Result<(), String>> {
        let tab = mux.read().get_tab(tab_id).cloned()?;
        let gate = self.state.lock().gates.get(tab_id).cloned();
        match gate {
            Some(gate) => match launch_gate_ready(&tab, &gate) {
                Ok(true) => Some(Ok(())),
                Ok(false) => None,
                Err(error) => Some(Err(error)),
            },
            None => ready_without_launch_gate(&tab),
        }
    }

    fn await_ready(
        &self,
        mux: &crate::MuxState,
        tab_id: &str,
        timeout: Duration,
    ) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        let mut state = self.state.lock();
        loop {
            let generation = state.generation;
            drop(state);
            if let Some(observed) = self.inspect(mux, tab_id) {
                return observed;
            }
            state = self.state.lock();
            if state.generation != generation {
                continue;
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return Err(format!(
                    "Tab {tab_id} public input readiness deadline elapsed; no input was written"
                ));
            };
            if self.changed.wait_for(&mut state, remaining).timed_out() {
                return Err(format!(
                    "Tab {tab_id} public input readiness deadline elapsed; no input was written"
                ));
            }
        }
    }
}

pub(crate) fn note_bootstrap_turn(
    mux: &crate::MuxState,
    tab_id: &str,
    session_id: &str,
    binding_nonce: &str,
    messages: &[crate::agent::chat_types::ChatMsg],
) -> bool {
    READINESS.note_turn(mux, tab_id, session_id, binding_nonce, messages)
}

pub(crate) fn notify_lifecycle_changed() {
    READINESS.notify();
}

impl ServiceContext {
    pub(crate) fn arm_public_send_launch(
        &self,
        attempt: LaunchAttempt,
        startup_turn_required: bool,
    ) {
        READINESS.arm(attempt, startup_turn_required);
    }

    pub(crate) fn await_public_send_readiness(
        &self,
        tab_id: &str,
        timeout: Duration,
    ) -> Result<(), String> {
        READINESS.await_ready(&self.mux, tab_id, timeout)
    }

    pub(crate) fn retire_public_send_launch(&self, tab_id: &str) {
        READINESS.retire(tab_id);
    }
}
