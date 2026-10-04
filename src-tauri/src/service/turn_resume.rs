//! Resume after restart: the one owner of the system continue a tab receives
//! when Domios stopped while its agent was mid-turn. Live turn edges keep a
//! durable mark on the tab; the next start settles each owed mark exactly once,
//! delivering only after the tab's own resumed session is agent-ready.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use codeterm_types::pty::{BootStage, TimelineState};
use parking_lot::{Condvar, Mutex};

use crate::multiplexer::commands::MuxState;
use crate::multiplexer::types::{
    observe_turn, request_continue, settle_turn_resume, withdraw_sent_claim, Tab, TabBacking,
    TurnResumeOutcome,
};

use super::ServiceContext;

#[cfg(test)]
mod tests;

pub(crate) const DEFAULT_CONTINUE: &str =
    "Domios restarted while you were working. Continue the work you were doing.";
/// Bounded failure deadline for owed tabs to come back agent-ready; never readiness.
const READY_DEADLINE: Duration = Duration::from_secs(300);

pub(crate) const DISABLED: &str = "disabled";
pub(crate) const NO_PTY_AGENT_SESSION: &str = "no_pty_agent_session";
pub(crate) const NO_AGENT_SESSION: &str = "no_agent_session";
pub(crate) const SPAWN_FAILED: &str = "spawn_failed";
pub(crate) const SESSION_NOT_RESUMED: &str = "session_not_resumed";
pub(crate) const SESSION_UNVERIFIED: &str = "session_unverified";
pub(crate) const AGENT_NOT_READY: &str = "agent_not_ready";
pub(crate) const CLAIM_NOT_PERSISTED: &str = "claim_not_persisted";
pub(crate) const SESSION_CHANGED: &str = "session_changed_before_send";
pub(crate) const SEND_REFUSED: &str = "send_refused";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Claim {
    pub session_id: String,
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Step {
    /// Nothing owed: never marked, already settled, or the tab is gone.
    Settled,
    Waiting,
    Skipped(&'static str),
    Claimed(Claim),
}

/// Agent-ready on its own nonce-verified binding of `session_id`.
pub(crate) fn ready_on_own_session(tab: &Tab, session_id: &str) -> bool {
    matches!(tab.backing, TabBacking::Pty)
        && tab.pty_id.is_some()
        && tab.boot_stage == Some(BootStage::AgentReady)
        && tab.launch_session_authenticated()
        && tab.agent_session_id.as_deref() == Some(session_id)
}

/// The tab's own session while its PTY agent is mid-turn.
pub(crate) fn working_session(tab: &Tab) -> Option<&str> {
    let working = tab
        .agent_state
        .as_ref()
        .is_some_and(|state| state.state == TimelineState::Working);
    if !working || !matches!(tab.backing, TabBacking::Pty) {
        return None;
    }
    tab.agent_session_id.as_deref()
}

pub(crate) fn record_turn_edge(tab: &mut Tab, start_epoch: &str) -> bool {
    let session = working_session(tab).map(str::to_string);
    observe_turn(&mut tab.turn_resume, session.as_deref(), start_epoch)
}

pub(crate) fn owed_tab_ids(mux: &MuxState, start_epoch: &str) -> BTreeSet<String> {
    mux.read()
        .snapshot()
        .tabs
        .into_values()
        .filter(|tab| {
            tab.turn_resume
                .as_ref()
                .is_some_and(|mark| mark.owed_in(start_epoch))
        })
        .map(|tab| tab.id)
        .collect()
}

/// Advance one owed tab. The claim and its settlement commit under one mux
/// write, so exactly one caller per tab and start ever receives `Claimed`.
pub(crate) fn step(mux: &MuxState, tab_id: &str, start_epoch: &str) -> Step {
    let mut mux = mux.write();
    let Some(tab) = mux.get_tab_mut(tab_id) else {
        return Step::Settled;
    };
    let Some(mark) = tab
        .turn_resume
        .as_ref()
        .filter(|mark| mark.owed_in(start_epoch))
    else {
        return Step::Settled;
    };
    if !matches!(tab.backing, TabBacking::Pty) {
        return skip(tab, start_epoch, NO_PTY_AGENT_SESSION);
    }
    let ready = tab.boot_stage == Some(BootStage::AgentReady)
        && tab.pty_id.is_some()
        && tab.launch_session_authenticated();
    if !ready {
        return Step::Waiting;
    }
    if !ready_on_own_session(tab, &mark.session_id) {
        return skip(tab, start_epoch, SESSION_NOT_RESUMED);
    }
    let claim = Claim {
        session_id: mark.session_id.clone(),
        prompt: mark.prompt.clone(),
    };
    if settle_turn_resume(&mut tab.turn_resume, start_epoch, TurnResumeOutcome::Sent) {
        Step::Claimed(claim)
    } else {
        Step::Settled
    }
}

/// Settle an owed tab that never came back ready, naming what it lacked.
pub(crate) fn expire(mux: &MuxState, tab_id: &str, start_epoch: &str) -> Option<&'static str> {
    let mut mux = mux.write();
    let tab = mux.get_tab_mut(tab_id)?;
    let reason = if tab.agent_type.is_none() || tab.agent_session_id.is_none() {
        NO_AGENT_SESSION
    } else if tab.boot_stage == Some(BootStage::SpawnFailed) {
        SPAWN_FAILED
    } else if !tab.launch_session_authenticated() {
        SESSION_UNVERIFIED
    } else {
        AGENT_NOT_READY
    };
    matches!(skip(tab, start_epoch, reason), Step::Skipped(_)).then_some(reason)
}

fn skip(tab: &mut Tab, start_epoch: &str, reason: &'static str) -> Step {
    let outcome = TurnResumeOutcome::Skipped(reason.to_string());
    if settle_turn_resume(&mut tab.turn_resume, start_epoch, outcome) {
        Step::Skipped(reason)
    } else {
        Step::Settled
    }
}

/// The owed tabs of this start, drained as each settles.
pub(crate) struct Wave {
    start_epoch: &'static str,
    pending: Mutex<BTreeSet<String>>,
    drained: Condvar,
}

impl Wave {
    pub(crate) fn new(start_epoch: &'static str, owed: BTreeSet<String>) -> Self {
        Self {
            start_epoch,
            pending: Mutex::new(owed),
            drained: Condvar::new(),
        }
    }

    pub(crate) fn is_pending(&self, tab_id: &str) -> bool {
        self.pending.lock().contains(tab_id)
    }

    pub(crate) fn pending_ids(&self) -> Vec<String> {
        self.pending.lock().iter().cloned().collect()
    }

    pub(crate) fn finish(&self, tab_id: &str) {
        let mut pending = self.pending.lock();
        if pending.remove(tab_id) && pending.is_empty() {
            self.drained.notify_all();
        }
    }

    /// Wait for every owed tab to settle; returns those still owed at `deadline`.
    pub(crate) fn wait_drained(&self, deadline: Instant) -> Vec<String> {
        let mut pending = self.pending.lock();
        while !pending.is_empty() {
            if self.drained.wait_until(&mut pending, deadline).timed_out() {
                break;
            }
        }
        pending.iter().cloned().collect()
    }
}

static WAVE: std::sync::OnceLock<Wave> = std::sync::OnceLock::new();

/// Arm this start's wave after unconditional tab restore.
pub(crate) fn arm(svc: &ServiceContext) {
    let Some(start_epoch) = crate::crash::lifecycle::current_start_epoch() else {
        return;
    };
    let owed = owed_tab_ids(&svc.mux, start_epoch);
    if owed.is_empty() {
        return;
    }
    if !crate::config::load_config().agent.continue_after_restart {
        for tab_id in &owed {
            let mut mux = svc.mux.write();
            if let Some(tab) = mux.get_tab_mut(tab_id) {
                skip(tab, start_epoch, DISABLED);
            }
        }
        log::info!(
            "[turn-resume] disabled; {} owed continue(s) settled as skipped",
            owed.len()
        );
        svc.schedule_session_persist_debounced();
        return;
    }
    log::info!(
        "[turn-resume] start {start_epoch}: {} tab(s) owe one continue: {:?}",
        owed.len(),
        owed
    );
    let wave = WAVE.get_or_init(|| Wave::new(start_epoch, owed));
    for tab_id in wave.pending_ids() {
        svc.consider_turn_resume(&tab_id);
    }
    let service = svc.clone();
    let spawned = std::thread::Builder::new()
        .name("turn-resume-deadline".into())
        .spawn(move || {
            let expired = wave.wait_drained(Instant::now() + READY_DEADLINE);
            if crate::crash::shutdown::is_shutting_down() {
                return;
            }
            for tab_id in expired {
                if let Some(reason) = expire(&service.mux, &tab_id, wave.start_epoch) {
                    log::warn!("[turn-resume] tab={tab_id} skipped: {reason}");
                }
                wave.finish(&tab_id);
            }
            service.schedule_session_persist_debounced();
        });
    if let Err(error) = spawned {
        log::error!("[turn-resume] deadline thread failed to start: {error}");
    }
}

impl ServiceContext {
    /// Fold a live turn edge into the tab's durable mark. Frozen once a graceful
    /// shutdown is armed so teardown never erases the turn it interrupted.
    pub(crate) fn record_turn_resume_edge(&self, tab_id: &str) {
        if crate::crash::shutdown::is_shutting_down() {
            return;
        }
        let Some(start_epoch) = crate::crash::lifecycle::current_start_epoch() else {
            return;
        };
        let changed = {
            let mut mux = self.mux.write();
            mux.get_tab_mut(tab_id)
                .is_some_and(|tab| record_turn_edge(tab, start_epoch))
        };
        if changed {
            self.schedule_session_persist_debounced();
        }
    }

    /// Ask the next start to continue `tab_id` with `prompt`; the tab must run
    /// an agent session for the continue to have a session to resume.
    pub(crate) fn request_turn_resume(&self, tab_id: &str, prompt: &str) -> Result<(), String> {
        let start_epoch =
            crate::crash::lifecycle::current_start_epoch().ok_or("start epoch is not recorded")?;
        let mut mux = self.mux.write();
        let resolved = mux
            .resolve_tab_id(tab_id)
            .ok_or_else(|| format!("Tab {tab_id} not found"))?;
        let tab = mux
            .get_tab_mut(&resolved)
            .ok_or_else(|| format!("Tab {tab_id} not found"))?;
        if !matches!(tab.backing, TabBacking::Pty) || tab.agent_type.is_none() {
            return Err(NO_PTY_AGENT_SESSION.into());
        }
        let session_id = tab.agent_session_id.clone().ok_or(NO_AGENT_SESSION)?;
        request_continue(
            &mut tab.turn_resume,
            &session_id,
            start_epoch,
            prompt.to_string(),
        );
        Ok(())
    }

    /// Binding or readiness moved for `tab_id`: keep a working mark on the
    /// live session and settle an owed continue once the tab is agent-ready.
    pub(crate) fn on_turn_resume_binding_changed(&self, tab_id: &str) {
        self.record_turn_resume_edge(tab_id);
        self.consider_turn_resume(tab_id);
    }

    /// A start that is already stopping leaves owed marks for the next start.
    pub(crate) fn consider_turn_resume(&self, tab_id: &str) {
        let Some(wave) = WAVE.get() else {
            return;
        };
        if !wave.is_pending(tab_id) || crate::crash::shutdown::is_shutting_down() {
            return;
        }
        match step(&self.mux, tab_id, wave.start_epoch) {
            Step::Waiting => {}
            Step::Settled => wave.finish(tab_id),
            Step::Skipped(reason) => {
                log::warn!("[turn-resume] tab={tab_id} skipped: {reason}");
                wave.finish(tab_id);
                self.schedule_session_persist_debounced();
            }
            Step::Claimed(claim) => {
                wave.finish(tab_id);
                self.dispatch_turn_resume(tab_id, claim, wave.start_epoch);
            }
        }
    }

    fn dispatch_turn_resume(&self, tab_id: &str, claim: Claim, start_epoch: &'static str) {
        let service = self.clone();
        let tab = tab_id.to_string();
        let spawned = std::thread::Builder::new()
            .name("turn-resume".into())
            .spawn(move || service.deliver_turn_resume(&tab, claim, start_epoch));
        if let Err(error) = spawned {
            log::error!("[turn-resume] tab={tab_id} delivery thread failed to start: {error}");
            self.withdraw_turn_resume(tab_id, start_epoch, "delivery_thread_unavailable");
        }
    }

    /// The claim is made durable before the payload is written, so a crash
    /// after the write can never deliver a second continue for this turn.
    fn deliver_turn_resume(&self, tab_id: &str, claim: Claim, start_epoch: &'static str) {
        let durable = matches!(
            super::session_persist::write_session_snapshot(&self.app_handle, &self.mux),
            Ok(crate::session::store::SaveOutcome::Saved)
        );
        if !durable {
            log::warn!("[turn-resume] tab={tab_id} skipped: {CLAIM_NOT_PERSISTED}");
            self.withdraw_turn_resume(tab_id, start_epoch, CLAIM_NOT_PERSISTED);
            return;
        }
        let own = self
            .mux
            .read()
            .get_tab(tab_id)
            .is_some_and(|tab| ready_on_own_session(tab, &claim.session_id));
        if !own {
            log::warn!("[turn-resume] tab={tab_id} skipped: {SESSION_CHANGED}");
            self.withdraw_turn_resume(tab_id, start_epoch, SESSION_CHANGED);
            return;
        }
        let body = claim.prompt.as_deref().unwrap_or(DEFAULT_CONTINUE);
        match super::messenger::Messenger::deliver(
            self,
            tab_id,
            super::messenger::MessageEnvelope::from_codeterm(),
            body,
        ) {
            Ok(_) => log::info!(
                "[turn-resume] tab={tab_id} session={} continue handed to the post-ready send path",
                claim.session_id
            ),
            Err(error) => {
                log::warn!(
                    "[turn-resume] tab={tab_id} session={} continue refused before any write: {error}",
                    claim.session_id
                );
                self.withdraw_turn_resume(tab_id, start_epoch, SEND_REFUSED);
            }
        }
    }

    fn withdraw_turn_resume(&self, tab_id: &str, start_epoch: &str, reason: &str) {
        {
            let mut mux = self.mux.write();
            if let Some(tab) = mux.get_tab_mut(tab_id) {
                withdraw_sent_claim(&mut tab.turn_resume, start_epoch, reason.to_string());
            }
        }
        self.schedule_session_persist_debounced();
    }
}
