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
    observe_turn, request_continue_for_restored_start, request_continue_from, settle_turn_resume,
    suppress_continue, withdraw_sent_claim, Tab, TabBacking, TurnResumeOutcome, TurnResumeSender,
};

use super::ServiceContext;

#[cfg(test)]
mod tests;

pub const DEFAULT_CONTINUE: &str =
    "Domios restarted while you were working. Continue the work you were doing.";
/// Bounded failure deadline for owed tabs to come back agent-ready; never readiness.
const READY_DEADLINE: Duration = Duration::from_secs(300);

pub const DISABLED: &str = "disabled";
pub(crate) const NO_PTY_AGENT_SESSION: &str = "no_pty_agent_session";
pub(crate) const NO_AGENT_SESSION: &str = "no_agent_session";
pub(crate) const SPAWN_FAILED: &str = "spawn_failed";
pub(crate) const SESSION_NOT_RESUMED: &str = "session_not_resumed";
pub(crate) const SESSION_UNVERIFIED: &str = "session_unverified";
pub(crate) const AGENT_NOT_READY: &str = "agent_not_ready";
pub(crate) const CLAIM_NOT_PERSISTED: &str = "claim_not_persisted";
pub(crate) const SESSION_CHANGED: &str = "session_changed_before_send";
pub(crate) const SEND_REFUSED: &str = "send_refused";
pub const CALLER_TAB_REQUIRED: &str = "caller_tab_required";
pub const NOT_CALLER_TAB: &str = "not_caller_tab";
pub(crate) const AFTER_UPDATE_NOT_PERSISTED: &str = "after_update_not_persisted";
pub(crate) const AFTER_UPDATE_NOT_CONSUMED: &str = "after_update_not_consumed";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Claim {
    pub session_id: String,
    pub prompt: Option<String>,
    pub sender: Option<TurnResumeSender>,
    pub pty_id: String,
    pub binding_nonce: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Step {
    /// Nothing owed: never marked, already settled, or the tab is gone.
    Settled,
    Waiting,
    Skipped(&'static str),
    Claimed(Claim),
}

#[derive(Debug, PartialEq, Eq)]
enum DeliveryFailure {
    ClaimNotPersisted,
    SessionChanged,
    SendRefused(String),
}

fn persist_then_send(
    persist: impl FnOnce() -> bool,
    binding_matches: impl FnOnce() -> bool,
    send: impl FnOnce() -> Result<(), String>,
) -> Result<(), DeliveryFailure> {
    if !persist() {
        return Err(DeliveryFailure::ClaimNotPersisted);
    }
    if !binding_matches() {
        return Err(DeliveryFailure::SessionChanged);
    }
    send().map_err(DeliveryFailure::SendRefused)
}

fn deliver_claim(
    claim: &Claim,
    persist: impl FnOnce() -> bool,
    binding_matches: impl FnOnce() -> bool,
    send: impl FnOnce(&str, &Claim) -> Result<(), String>,
) -> Result<(), DeliveryFailure> {
    let body = claim.prompt.as_deref().unwrap_or(DEFAULT_CONTINUE);
    persist_then_send(persist, binding_matches, || send(body, claim))
}

fn ready_binding_on_own_session(tab: &Tab, session_id: &str) -> Option<(String, String)> {
    if !matches!(tab.backing, TabBacking::Pty)
        || tab.boot_stage != Some(BootStage::AgentReady)
        || !tab.launch_session_authenticated()
        || tab.agent_session_id.as_deref() != Some(session_id)
    {
        return None;
    }
    Some((tab.pty_id.clone()?, tab.session_binding_nonce.clone()?))
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

pub(crate) fn record_turn_resume_edge_if_active(
    tab: &mut Tab,
    start_epoch: &str,
    shutting_down: bool,
    state_changed: bool,
) -> bool {
    state_changed && !shutting_down && record_turn_edge(tab, start_epoch)
}

fn request_enabled(continue_after_restart: bool) -> Result<(), &'static str> {
    continue_after_restart.then_some(()).ok_or(DISABLED)
}

fn request_targets_caller(caller_tab_id: &str, target_tab_id: &str) -> Result<(), &'static str> {
    (caller_tab_id == target_tab_id)
        .then_some(())
        .ok_or(NOT_CALLER_TAB)
}

fn settle_disabled_marks(mux: &MuxState, owed: &BTreeSet<String>, start_epoch: &str) -> usize {
    owed.iter()
        .filter(|tab_id| {
            let mut mux = mux.write();
            mux.get_tab_mut(tab_id)
                .is_some_and(|tab| matches!(skip(tab, start_epoch, DISABLED), Step::Skipped(_)))
        })
        .count()
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
    if tab.boot_stage != Some(BootStage::AgentReady)
        || tab.pty_id.is_none()
        || !tab.launch_session_authenticated()
    {
        return Step::Waiting;
    }
    let Some((pty_id, binding_nonce)) = ready_binding_on_own_session(tab, &mark.session_id) else {
        return skip(tab, start_epoch, SESSION_NOT_RESUMED);
    };
    let claim = Claim {
        session_id: mark.session_id.clone(),
        prompt: mark.prompt.clone(),
        sender: mark.sender.clone(),
        pty_id,
        binding_nonce,
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

    pub fn is_pending(&self, tab_id: &str) -> bool {
        self.pending.lock().contains(tab_id)
    }

    pub(crate) fn pending_ids(&self) -> Vec<String> {
        self.pending.lock().iter().cloned().collect()
    }

    pub fn finish(&self, tab_id: &str) {
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

fn advance_consider(
    mux: &MuxState,
    tab_id: &str,
    wave: &Wave,
    shutting_down: bool,
) -> Option<Step> {
    if !wave.is_pending(tab_id) || shutting_down {
        return None;
    }
    let result = step(mux, tab_id, wave.start_epoch);
    if !matches!(&result, Step::Waiting) {
        wave.finish(tab_id);
    }
    Some(result)
}

#[derive(Debug, PartialEq, Eq)]
enum DeadlineOutcome {
    Retained,
    Expired(Option<&'static str>),
}

fn settle_deadline(
    mux: &MuxState,
    tab_id: &str,
    wave: &Wave,
    provider_live: bool,
) -> DeadlineOutcome {
    if provider_live {
        return DeadlineOutcome::Retained;
    }
    let reason = expire(mux, tab_id, wave.start_epoch);
    wave.finish(tab_id);
    DeadlineOutcome::Expired(reason)
}

static WAVE: std::sync::OnceLock<Wave> = std::sync::OnceLock::new();

/// Arm this start's wave after unconditional tab restore.
pub fn arm(svc: &ServiceContext) {
    let Some(start_epoch) = crate::crash::lifecycle::current_start_epoch() else {
        return;
    };
    let pending_update = crate::service::post_update::pending();
    if let Err((tab_id, reason)) = arm_after_update_message(
        &svc.mux,
        start_epoch,
        pending_update.as_ref(),
        || {
            matches!(
                super::session_persist::write_session_snapshot(&svc, &svc.mux),
                Ok(crate::session::store::SaveOutcome::Saved)
            )
        },
        || crate::service::post_update::consume().is_ok(),
    ) {
        log::warn!("[turn-resume] tab={tab_id} outcome=skipped reason={reason}");
        svc.schedule_session_persist_debounced();
    }
    let owed = owed_tab_ids(&svc.mux, start_epoch);
    if owed.is_empty() {
        return;
    }
    if !svc.configuration().agent.continue_after_restart {
        settle_disabled_marks(&svc.mux, &owed, start_epoch);
        log::info!(
            "[turn-resume] disabled; {} owed continue(s) settled as skipped",
            owed.len()
        );
        if let Err(error) = super::session_persist::write_session_snapshot(&svc, &svc.mux) {
            log::warn!("[turn-resume] disabled marks were not persisted: {error}");
        }
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
                let pty_id = service.mux.read().get_tab(&tab_id).and_then(|tab| tab.pty_id.clone());
                let provider_live = pty_id.is_some_and(|pty| service.resumed_provider_is_live(&tab_id, &pty));
                match settle_deadline(&service.mux, &tab_id, wave, provider_live) {
                    DeadlineOutcome::Retained => {
                        log::warn!("[turn-resume] tab={tab_id} outcome=skipped reason=live_binding_pending owed_continue=retained");
                    }
                    DeadlineOutcome::Expired(reason) => {
                        log::warn!("[turn-resume] tab={tab_id} outcome=expired reason={}", reason.unwrap_or("tab_gone_or_settled"));
                    }
                }
            }
            service.schedule_session_persist_debounced();
        });
    if let Err(error) = spawned {
        log::error!("[turn-resume] deadline thread failed to start: {error}");
    }
}

fn arm_after_update_message(
    mux: &MuxState,
    start_epoch: &str,
    message: Option<&crate::service::post_update::PendingMessage>,
    persist: impl FnOnce() -> bool,
    consume: impl FnOnce() -> bool,
) -> Result<Option<String>, (String, &'static str)> {
    let Some(message) = message else {
        return Ok(None);
    };
    let Some(tab_id) = stage_after_update_prompt(mux, start_epoch, message) else {
        return Ok(None);
    };
    let reason = if !persist() {
        Some(AFTER_UPDATE_NOT_PERSISTED)
    } else if !consume() {
        Some(AFTER_UPDATE_NOT_CONSUMED)
    } else {
        None
    };
    if let Some(reason) = reason {
        let mut mux = mux.write();
        if let Some(tab) = mux.get_tab_mut(&tab_id) {
            skip(tab, start_epoch, reason);
        }
        Err((tab_id, reason))
    } else {
        Ok(Some(tab_id))
    }
}

fn stage_after_update_prompt(
    mux: &MuxState,
    start_epoch: &str,
    message: &crate::service::post_update::PendingMessage,
) -> Option<String> {
    let mut mux = mux.write();
    let tab_id = mux.resolve_tab_id(crate::service::general_agent::GENERAL_ALIAS)?;
    let tab = mux.get_tab_mut(&tab_id)?;
    if !matches!(tab.backing, TabBacking::Pty) || tab.agent_type.is_none() {
        return None;
    }
    let session_id = tab
        .agent_session_id
        .clone()
        .or_else(|| tab.turn_resume.as_ref().map(|mark| mark.session_id.clone()))?;
    request_continue_for_restored_start(
        &mut tab.turn_resume,
        &session_id,
        start_epoch,
        message.text.clone(),
        message.sender.clone(),
    );
    Some(tab_id)
}

impl ServiceContext {
    /// Fold a live turn edge into the tab's durable mark. Frozen once a graceful
    /// shutdown is armed so teardown never erases the turn it interrupted.
    pub(crate) fn record_turn_resume_edge(&self, tab_id: &str) {
        let Some(start_epoch) = crate::crash::lifecycle::current_start_epoch() else {
            return;
        };
        let changed = {
            let mut mux = self.mux.write();
            mux.get_tab_mut(tab_id).is_some_and(|tab| {
                record_turn_resume_edge_if_active(
                    tab,
                    start_epoch,
                    crate::crash::shutdown::is_shutting_down(),
                    true,
                )
            })
        };
        if changed {
            self.schedule_session_persist_debounced();
        }
    }

    /// Ask the next start to continue the authenticated caller's own session.
    pub fn request_turn_resume_for_caller(
        &self,
        caller_tab_id: &str,
        tab_id: &str,
        prompt: &str,
    ) -> Result<(), String> {
        request_enabled(self.configuration().agent.continue_after_restart)?;
        let start_epoch =
            crate::crash::lifecycle::current_start_epoch().ok_or("start epoch is not recorded")?;
        let mut mux = self.mux.write();
        let resolved = mux
            .resolve_tab_id(tab_id)
            .ok_or_else(|| format!("Tab {tab_id} not found"))?;
        let tab = mux
            .get_tab_mut(&resolved)
            .ok_or_else(|| format!("Tab {tab_id} not found"))?;
        request_targets_caller(caller_tab_id, &resolved)?;
        if !matches!(tab.backing, TabBacking::Pty) || tab.agent_type.is_none() {
            return Err(NO_PTY_AGENT_SESSION.into());
        }
        let session_id = tab.agent_session_id.clone().ok_or(NO_AGENT_SESSION)?;
        let sender = TurnResumeSender {
            tab_id: caller_tab_id.to_string(),
            label: tab.title.clone(),
            from_agent: Some(true),
            from_model: tab.agent_session_model.clone(),
        };
        request_continue_from(
            &mut tab.turn_resume,
            &session_id,
            start_epoch,
            prompt.to_string(),
            sender,
        );
        Ok(())
    }

    pub fn suppress_turn_resume_for_caller(&self, caller_tab_id: &str) -> Result<bool, String> {
        let start_epoch =
            crate::crash::lifecycle::current_start_epoch().ok_or("start epoch is not recorded")?;
        let changed = {
            let mut mux = self.mux.write();
            let tab = mux
                .get_tab_mut(caller_tab_id)
                .ok_or_else(|| format!("Tab {caller_tab_id} not found"))?;
            if !tab
                .turn_resume
                .as_ref()
                .is_some_and(|mark| mark.owed_in(start_epoch))
                && working_session(tab).is_none()
            {
                false
            } else {
                let session_id = tab
                    .agent_session_id
                    .clone()
                    .or_else(|| tab.turn_resume.as_ref().map(|mark| mark.session_id.clone()))
                    .unwrap_or_default();
                suppress_continue(&mut tab.turn_resume, &session_id, start_epoch)
            }
        };
        if changed {
            self.schedule_session_persist_debounced();
        }
        Ok(changed)
    }

    pub fn withdraw_turn_resume_request(
        &self,
        caller_tab_id: &str,
        tab_id: &str,
        prompt: &str,
    ) -> Result<bool, String> {
        request_targets_caller(caller_tab_id, tab_id)?;
        let start_epoch =
            crate::crash::lifecycle::current_start_epoch().ok_or("start epoch is not recorded")?;
        let changed = {
            let mut mux = self.mux.write();
            let tab = mux
                .get_tab_mut(tab_id)
                .ok_or_else(|| format!("Tab {tab_id} not found"))?;
            let is_request = tab.turn_resume.as_ref().is_some_and(|mark| {
                mark.start_epoch == start_epoch
                    && mark.prompt.as_deref() == Some(prompt)
                    && mark.settled.is_none()
            });
            if !is_request {
                false
            } else {
                tab.turn_resume = None;
                record_turn_edge(tab, start_epoch)
            }
        };
        if changed {
            self.schedule_session_persist_debounced();
        }
        Ok(changed)
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
        let Some(result) = advance_consider(
            &self.mux,
            tab_id,
            wave,
            crate::crash::shutdown::is_shutting_down(),
        ) else {
            return;
        };
        match result {
            Step::Waiting => {}
            Step::Settled => {
                log::info!("[turn-resume] tab={tab_id} outcome=skipped reason=no_owed_continue_or_tab_gone");
            }
            Step::Skipped(reason) => {
                log::warn!("[turn-resume] tab={tab_id} outcome=skipped reason={reason}");
                self.schedule_session_persist_debounced();
            }
            Step::Claimed(claim) => {
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
        let delivery = deliver_claim(
            &claim,
            || {
                matches!(
                    super::session_persist::write_session_snapshot(self, &self.mux),
                    Ok(crate::session::store::SaveOutcome::Saved)
                )
            },
            || {
                self.mux.read().get_tab(tab_id).is_some_and(|tab| {
                    ready_binding_on_own_session(tab, &claim.session_id)
                        == Some((claim.pty_id.clone(), claim.binding_nonce.clone()))
                })
            },
            |body, claim| {
                super::messenger::Messenger::deliver_turn_resume(
                    self,
                    tab_id,
                    &claim.pty_id,
                    &claim.session_id,
                    &claim.binding_nonce,
                    claim.sender.as_ref(),
                    body,
                )
                .map(|_| ())
            },
        );
        match delivery {
            Ok(()) => log::info!(
                "[turn-resume] tab={tab_id} session={} outcome=delivered evidence=write_handoff",
                claim.session_id
            ),
            Err(DeliveryFailure::ClaimNotPersisted) => {
                log::warn!(
                    "[turn-resume] tab={tab_id} outcome=skipped reason={CLAIM_NOT_PERSISTED}"
                );
                self.withdraw_turn_resume(tab_id, start_epoch, CLAIM_NOT_PERSISTED);
            }
            Err(DeliveryFailure::SessionChanged) => {
                log::warn!("[turn-resume] tab={tab_id} outcome=skipped reason={SESSION_CHANGED}");
                self.withdraw_turn_resume(tab_id, start_epoch, SESSION_CHANGED);
            }
            Err(DeliveryFailure::SendRefused(error)) => {
                if crate::service::pty_io::delivery_is_unknown(&error) {
                    log::warn!(
                        "[turn-resume] tab={tab_id} session={} outcome=skipped reason=submit_unconfirmed error={error}",
                        claim.session_id
                    );
                } else {
                    log::warn!(
                        "[turn-resume] tab={tab_id} session={} outcome=skipped reason=send_refused error={error}",
                        claim.session_id
                    );
                    self.withdraw_turn_resume(tab_id, start_epoch, SEND_REFUSED);
                }
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
