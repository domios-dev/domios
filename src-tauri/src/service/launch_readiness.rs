//! One owner for launch readiness. The General Agent start and ordinary agent
//! spawn arm the same bounded observation, bind it to the tab's own PTY and
//! launch attempt, and settle it in the existing boot-stage vocabulary: a
//! proven pre-readiness process exit is a failure, silence never is.

use super::ServiceContext;
use crate::multiplexer::types::Tab;
use codeterm_types::pty::BootStage;
use parking_lot::{Condvar, Mutex};
use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Instant;

#[cfg(test)]
#[path = "launch_readiness/tests.rs"]
mod tests;

/// The launch command finished before any readiness evidence: the process this
/// launch started is gone, so the launch failed.
pub(crate) const EXITED_BEFORE_READINESS: &str = "provider process exited before launch readiness";
/// The General Agent's own output deadline, unchanged, kept here so both paths
/// name their provisional evidence from one vocabulary.
pub(crate) const GENERAL_AGENT_OUTPUT_DEADLINE: &str = "agent output deadline elapsed";
/// An ordinary spawn's readiness deadline: this observer gave up, nothing more.
pub(crate) const SPAWN_READINESS_DEADLINE: &str = "launch readiness deadline elapsed";

/// The one launch an observation speaks for: the tab, the PTY it launched on,
/// and that launch's binding nonce. A prior PTY or a rotated nonce is a
/// different attempt and can never settle this one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LaunchAttempt {
    pub(crate) tab_id: String,
    pub(crate) pty_id: String,
    pub(crate) binding_nonce: String,
}

impl LaunchAttempt {
    /// The attempt as the tab itself currently holds it, or `None` once the tab
    /// has moved on.
    pub(crate) fn current(mux: &crate::MuxState, tab_id: &str, pty_id: &str) -> Option<Self> {
        let mux = mux.read();
        let tab = mux.get_tab(tab_id)?;
        if tab.pty_id.as_deref() != Some(pty_id) {
            return None;
        }
        Some(Self {
            tab_id: tab_id.to_string(),
            pty_id: pty_id.to_string(),
            binding_nonce: tab.session_binding_nonce.clone()?,
        })
    }
}

/// What one bounded observation learned about a launch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LaunchEvidence {
    /// The agent named itself on this launch: a nonce-verified session binding.
    Acknowledged,
    /// The launch command finished before readiness, carrying its exit code.
    ProcessExited { exit_code: Option<i32> },
    /// The observer gave up, carrying the provisional stage and the evidence
    /// that made it give up. Uncertainty about this attempt, never a verdict.
    Unobserved { stage: BootStage, reason: String },
}

/// How the shared owner records one piece of evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LaunchSettlement {
    Ready,
    AwaitingFirstTurn,
    Failed {
        reason: String,
        exit_code: Option<i32>,
    },
    Provisional {
        stage: BootStage,
        reason: String,
    },
    /// The tab left this launch behind: nothing to record.
    Superseded,
}

impl LaunchSettlement {
    pub(crate) fn authorizes_agent_ready(&self, session_authenticated: bool) -> bool {
        session_authenticated && matches!(self, Self::Ready | Self::Provisional { .. })
    }
}

/// A launch failure already decided against the launch it names, waiting to be
/// recorded once its observer has finished whatever teardown it performs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LaunchExitFailure {
    attempt: LaunchAttempt,
    exit_code: Option<i32>,
    reason: String,
}

/// The result of waiting on one armed observation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LaunchObservation {
    Evidence(LaunchEvidence),
    /// The bounded deadline elapsed with nothing observed.
    Deadline,
    /// Somebody else already consumed this observation.
    Retired,
}

/// Whether the tab still holds the exact launch this observation is bound to.
pub(crate) fn attempt_owns_tab(tab: &Tab, attempt: &LaunchAttempt) -> bool {
    tab.pty_id.as_deref() == Some(attempt.pty_id.as_str())
        && tab.session_binding_nonce.as_deref() == Some(attempt.binding_nonce.as_str())
}

fn resume_attempt_is_waiting_for_first_turn(tab: &Tab, attempt: &LaunchAttempt) -> bool {
    attempt_owns_tab(tab, attempt)
        && tab.boot_stage == Some(BootStage::AgentReady)
        && tab.exit_suspected_at.is_none()
}

/// Names the tab and its launch attempt, so a caller or the UI can act on the
/// failure without reading a byte of terminal output.
pub(crate) fn exited_before_readiness_reason(
    attempt: &LaunchAttempt,
    exit_code: Option<i32>,
) -> String {
    let code = match exit_code {
        Some(code) => code.to_string(),
        None => "unknown".to_string(),
    };
    format!(
        "{EXITED_BEFORE_READINESS}: tab={} attempt={} launch_nonce={} exit_code={code}",
        attempt.tab_id, attempt.pty_id, attempt.binding_nonce
    )
}

/// The whole classification, as a pure rule over the tab's own record. A live
/// agent's own binding proof still outranks this failure — `honest_boot_stage`
/// owns that rule and applies it when the stage is recorded.
pub(crate) fn settle_evidence(
    tab: &Tab,
    attempt: &LaunchAttempt,
    evidence: LaunchEvidence,
) -> LaunchSettlement {
    if !attempt_owns_tab(tab, attempt) {
        return LaunchSettlement::Superseded;
    }
    match evidence {
        LaunchEvidence::Acknowledged => LaunchSettlement::Ready,
        LaunchEvidence::ProcessExited { exit_code } => LaunchSettlement::Failed {
            reason: exited_before_readiness_reason(attempt, exit_code),
            exit_code,
        },
        LaunchEvidence::Unobserved { stage, reason } => {
            debug_assert!(stage.is_provisional());
            LaunchSettlement::Provisional { stage, reason }
        }
    }
}

struct Armed {
    attempt: LaunchAttempt,
    evidence: Option<LaunchEvidence>,
    /// One observer already took this exit. Both exit sources claim before
    /// either publishes, so the published verdict cannot be the dedup key.
    exit_claimed: bool,
    /// A verdict already published for this attempt. Kept so the launch thread
    /// can see it and never claims ready over a launch known to be gone.
    failure: Option<String>,
}

struct Observations {
    armed: Mutex<HashMap<String, Armed>>,
    changed: Condvar,
}

static OBSERVATIONS: LazyLock<Observations> = LazyLock::new(|| Observations {
    armed: Mutex::new(HashMap::new()),
    changed: Condvar::new(),
});

/// Arm the bounded observation for one launch attempt. Replaces any earlier
/// observation for the tab: only the newest launch is observable.
pub(crate) fn arm(attempt: LaunchAttempt) {
    OBSERVATIONS.armed.lock().insert(
        attempt.tab_id.clone(),
        Armed {
            attempt,
            evidence: None,
            exit_claimed: false,
            failure: None,
        },
    );
    // Releases any observer still waiting on the launch this one supersedes.
    OBSERVATIONS.changed.notify_all();
}

/// Record evidence against the armed observation for this exact PTY, once the
/// source proves the evidence belongs to this launch. Returns false for a
/// foreign or superseded launch, which therefore cannot settle this attempt.
fn note(
    tab_id: &str,
    pty_id: &str,
    evidence: LaunchEvidence,
    owns_attempt: impl Fn(&LaunchAttempt) -> bool,
) -> bool {
    let mut armed = OBSERVATIONS.armed.lock();
    let Some(observed) = armed.get_mut(tab_id) else {
        return false;
    };
    if observed.attempt.pty_id != pty_id
        || !owns_attempt(&observed.attempt)
        || observed.evidence.is_some()
        || observed.failure.is_some()
    {
        return false;
    }
    observed.evidence = Some(evidence);
    OBSERVATIONS.changed.notify_all();
    true
}

/// The agent proved this launch: a nonce-verified provider session binding.
pub(crate) fn note_acknowledged(tab_id: &str, pty_id: &str, binding_nonce: &str) -> bool {
    note(tab_id, pty_id, LaunchEvidence::Acknowledged, |attempt| {
        attempt.binding_nonce == binding_nonce
    })
}

/// Take this tab's launch for an exit observed right now. One lock does the
/// whole claim, so the observer waiting on this launch cannot take it away
/// mid-claim. It asks nothing of the shell's reported command line: the
/// observation is already bound to one PTY and one launch nonce, and a shell
/// that reports an empty command line is the ordinary case, not a foreign
/// command. An acknowledged or already-failed launch is refused.
fn claim_exit(
    tab_id: &str,
    pty_id: Option<&str>,
    exit_code: Option<i32>,
) -> Option<(LaunchAttempt, Option<i32>)> {
    let mut armed = OBSERVATIONS.armed.lock();
    let observed = armed.get_mut(tab_id)?;
    if pty_id.is_some_and(|pty_id| pty_id != observed.attempt.pty_id)
        || observed.exit_claimed
        || observed.failure.is_some()
        || matches!(observed.evidence, Some(LaunchEvidence::Acknowledged))
    {
        return None;
    }
    observed.exit_claimed = true;
    let exit_code = match observed.evidence.take() {
        Some(LaunchEvidence::ProcessExited { exit_code }) => exit_code,
        _ => exit_code,
    };
    Some((observed.attempt.clone(), exit_code))
}

/// Mark this attempt's verdict as published.
fn record_published_failure(attempt: &LaunchAttempt, reason: &str) {
    let mut armed = OBSERVATIONS.armed.lock();
    if let Some(observed) = armed.get_mut(&attempt.tab_id) {
        if &observed.attempt == attempt {
            observed.evidence = None;
            observed.failure = Some(reason.to_string());
        }
    }
    OBSERVATIONS.changed.notify_all();
}

/// Take the verdict already published for this attempt, so the launch thread
/// stops instead of claiming ready over a launch that is known to be gone.
pub(crate) fn take_published_failure(attempt: &LaunchAttempt) -> Option<String> {
    let mut armed = OBSERVATIONS.armed.lock();
    let observed = armed.get(&attempt.tab_id)?;
    if &observed.attempt != attempt {
        return None;
    }
    let reason = observed.failure.clone()?;
    armed.remove(&attempt.tab_id);
    Some(reason)
}

/// Wait for this launch's own readiness evidence until the bounded deadline.
/// Event-driven on the evidence notes; the deadline only bounds the failure to
/// observe anything and is never itself treated as readiness.
pub(crate) fn observe(attempt: &LaunchAttempt, deadline: Instant) -> LaunchObservation {
    let mut armed = OBSERVATIONS.armed.lock();
    loop {
        let Some(observed) = armed.get_mut(&attempt.tab_id) else {
            return LaunchObservation::Retired;
        };
        if &observed.attempt != attempt || observed.failure.is_some() {
            return LaunchObservation::Retired;
        }
        if let Some(evidence) = observed.evidence.take() {
            armed.remove(&attempt.tab_id);
            return LaunchObservation::Evidence(evidence);
        }
        if OBSERVATIONS
            .changed
            .wait_until(&mut armed, deadline)
            .timed_out()
        {
            armed.remove(&attempt.tab_id);
            return LaunchObservation::Deadline;
        }
    }
}

impl ServiceContext {
    /// Record one piece of launch-readiness evidence for both launch paths.
    pub(crate) fn settle_launch_readiness(
        &self,
        attempt: &LaunchAttempt,
        evidence: LaunchEvidence,
    ) -> LaunchSettlement {
        let settlement = {
            let mux = self.mux.read();
            match mux.get_tab(&attempt.tab_id) {
                Some(tab) => settle_evidence(tab, attempt, evidence),
                None => LaunchSettlement::Superseded,
            }
        };
        match &settlement {
            LaunchSettlement::Ready => log::info!(
                "[launch-readiness] Tab {} acknowledged its own launch on PTY {}",
                attempt.tab_id,
                attempt.pty_id
            ),
            LaunchSettlement::Superseded => log::info!(
                "[launch-readiness] Tab {} moved on from the launch on PTY {}; evidence ignored",
                attempt.tab_id,
                attempt.pty_id
            ),
            LaunchSettlement::AwaitingFirstTurn => log::info!(
                "[launch-readiness] Tab {} resume awaiting first turn on PTY {}",
                attempt.tab_id,
                attempt.pty_id
            ),
            LaunchSettlement::Provisional { stage, reason } => match stage {
                BootStage::LaunchDeadlineElapsed => {
                    self.record_launch_output_deadline(attempt, reason)
                }
                stage => self.record_provisional_launch(attempt, *stage, reason),
            },
            LaunchSettlement::Failed { reason, exit_code } => {
                self.publish_launch_exit(&LaunchExitFailure {
                    attempt: attempt.clone(),
                    exit_code: *exit_code,
                    reason: reason.clone(),
                });
            }
        }
        settlement
    }

    /// One exit observation: record it against the launch under observation and
    /// decide the verdict, both while the tab still carries that launch. Every
    /// observer that sees an exit also tears the tab's agent identity down, and
    /// the teardown clears the launch a verdict names, so the decision is taken
    /// here and published afterwards.
    pub(crate) fn claim_launch_exit(
        &self,
        tab_id: &str,
        pty_id: Option<&str>,
        exit_code: Option<i32>,
    ) -> Option<LaunchExitFailure> {
        let (attempt, exit_code) = claim_exit(tab_id, pty_id, exit_code)?;
        self.launch_exit_verdict(&attempt, exit_code)
    }

    fn launch_exit_verdict(
        &self,
        attempt: &LaunchAttempt,
        exit_code: Option<i32>,
    ) -> Option<LaunchExitFailure> {
        let mux = self.mux.read();
        let tab = mux.get_tab(&attempt.tab_id)?;
        // A live agent that proved this launch outranks one exit observer.
        if tab.launch_session_authenticated() {
            return None;
        }
        match settle_evidence(tab, attempt, LaunchEvidence::ProcessExited { exit_code }) {
            LaunchSettlement::Failed { reason, exit_code } => Some(LaunchExitFailure {
                attempt: attempt.clone(),
                exit_code,
                reason,
            }),
            _ => None,
        }
    }

    /// Record a claimed verdict. Safe after a teardown, because the verdict was
    /// decided while the launch it names was still the tab's own.
    pub(crate) fn publish_launch_exit(&self, failure: &LaunchExitFailure) {
        record_published_failure(&failure.attempt, &failure.reason);
        self.emit_launch_readiness_failure(&failure.attempt, &failure.reason, failure.exit_code);
    }

    /// The structured launch failure: identity and exit code, never terminal
    /// text, so the spawner and the UI can act on it directly.
    fn emit_launch_readiness_failure(
        &self,
        attempt: &LaunchAttempt,
        reason: &str,
        exit_code: Option<i32>,
    ) {
        log::error!(
            "[launch-readiness] Tab {} launch failed on PTY {}: {reason}",
            attempt.tab_id,
            attempt.pty_id
        );
        crate::api::events::emit_everywhere(
            &self.app_handle,
            "agent-launch-failed",
            serde_json::json!({
                "kind": "exited_before_readiness",
                "tab_id": attempt.tab_id,
                "attempt_id": attempt.pty_id,
                "launch_nonce": attempt.binding_nonce,
                "exit_code": exit_code,
                "reason": reason,
            }),
        );
        self.emit_boot_stage(&attempt.tab_id, BootStage::SpawnFailed, Some(reason));
    }

    /// Own one launch's bounded readiness observation to its end. Runs on its
    /// own thread so no launch blocks on it.
    pub(crate) fn observe_launch_readiness(
        &self,
        attempt: LaunchAttempt,
        deadline: std::time::Duration,
        resume_binds_on_first_user_turn: bool,
    ) -> std::sync::mpsc::Receiver<LaunchSettlement> {
        let (settled, receiver) = std::sync::mpsc::sync_channel(1);
        let svc = self.clone();
        std::thread::spawn(move || {
            let evidence = match observe(&attempt, Instant::now() + deadline) {
                LaunchObservation::Evidence(evidence) => evidence,
                LaunchObservation::Deadline
                    if resume_binds_on_first_user_turn
                        && svc.resume_launch_attempt_is_waiting_for_first_turn(&attempt) =>
                {
                    let authenticated = {
                        let mux = svc.mux.read();
                        mux.get_tab(&attempt.tab_id)
                            .is_some_and(|tab| tab.launch_session_authenticated())
                    };
                    if authenticated {
                        LaunchEvidence::Acknowledged
                    } else {
                        log::info!(
                            "[launch-readiness] Tab {} resume awaiting first turn on live PTY {}",
                            attempt.tab_id,
                            attempt.pty_id
                        );
                        let _ = settled.send(LaunchSettlement::AwaitingFirstTurn);
                        return;
                    }
                }
                LaunchObservation::Deadline => LaunchEvidence::Unobserved {
                    stage: BootStage::LaunchDeadlineElapsed,
                    reason: SPAWN_READINESS_DEADLINE.to_string(),
                },
                LaunchObservation::Retired => {
                    let settlement = take_published_failure(&attempt)
                        .map(|reason| LaunchSettlement::Failed {
                            reason,
                            exit_code: None,
                        })
                        .unwrap_or(LaunchSettlement::Superseded);
                    let _ = settled.send(settlement);
                    return;
                }
            };
            let hook_missed_deadline = matches!(evidence, LaunchEvidence::Unobserved { .. });
            let settlement = svc.settle_launch_readiness(&attempt, evidence);
            if hook_missed_deadline && settlement != LaunchSettlement::Superseded {
                svc.detect_unbound_launch_session(&attempt.tab_id);
            }
            let _ = settled.send(settlement);
        });
        receiver
    }

    fn resume_launch_attempt_is_waiting_for_first_turn(&self, attempt: &LaunchAttempt) -> bool {
        let awaiting_first_turn = {
            let mux = self.mux.read();
            mux.get_tab(&attempt.tab_id)
                .is_some_and(|tab| resume_attempt_is_waiting_for_first_turn(tab, attempt))
        };
        awaiting_first_turn && self.pty.lock().get_session(&attempt.pty_id).is_some()
    }
}
