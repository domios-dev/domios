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

/// The tab's shell reported a finished foreground command while this launch was
/// still short of readiness. The shell's own command line names this launch by
/// its nonce, so nothing else in the tab can be mistaken for it; a shell that
/// reports no command line is taken at its word about its only command.
pub(crate) fn note_launch_command_exit(
    tab_id: &str,
    pty_id: &str,
    command_line: Option<&str>,
    exit_code: Option<i32>,
) -> bool {
    note(
        tab_id,
        pty_id,
        LaunchEvidence::ProcessExited { exit_code },
        |attempt| match command_line {
            Some(line) => line.contains(&attempt.binding_nonce),
            None => true,
        },
    )
}

/// Take whatever this observation already holds, without waiting.
pub(crate) fn take_evidence(attempt: &LaunchAttempt) -> Option<LaunchEvidence> {
    let mut armed = OBSERVATIONS.armed.lock();
    let observed = armed.get_mut(&attempt.tab_id)?;
    if &observed.attempt != attempt {
        return None;
    }
    let evidence = observed.evidence.take()?;
    armed.remove(&attempt.tab_id);
    Some(evidence)
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
        if &observed.attempt != attempt {
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
            LaunchSettlement::Provisional { stage, reason } => match stage {
                BootStage::LaunchDeadlineElapsed => {
                    self.record_launch_output_deadline(&attempt.tab_id, &attempt.pty_id, reason)
                }
                stage => {
                    self.record_provisional_launch(&attempt.tab_id, &attempt.pty_id, *stage, reason)
                }
            },
            LaunchSettlement::Failed { reason, exit_code } => {
                self.emit_launch_readiness_failure(attempt, reason, *exit_code);
            }
        }
        settlement
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

    /// Settle an exit this launch already produced, before the launch thread
    /// claims readiness. Returns the failure reason when the process is gone.
    pub(crate) fn settle_observed_launch_exit(&self, attempt: &LaunchAttempt) -> Option<String> {
        let Some(evidence @ LaunchEvidence::ProcessExited { .. }) = take_evidence(attempt) else {
            return None;
        };
        let LaunchSettlement::Failed { reason, .. } =
            self.settle_launch_readiness(attempt, evidence)
        else {
            return None;
        };
        // Recording refuses a failure the tab's own live agent disproved; only
        // a failure that latched means this launch is genuinely gone.
        self.mux
            .read()
            .get_tab(&attempt.tab_id)
            .is_some_and(|tab| tab.boot_stage == Some(BootStage::SpawnFailed))
            .then_some(reason)
    }

    /// Own one launch's bounded readiness observation to its end. Runs on its
    /// own thread so no launch blocks on it.
    pub(crate) fn observe_launch_readiness(
        &self,
        attempt: LaunchAttempt,
        deadline: std::time::Duration,
    ) {
        let svc = self.clone();
        std::thread::spawn(move || {
            let evidence = match observe(&attempt, Instant::now() + deadline) {
                LaunchObservation::Evidence(evidence) => evidence,
                LaunchObservation::Deadline => LaunchEvidence::Unobserved {
                    stage: BootStage::LaunchDeadlineElapsed,
                    reason: SPAWN_READINESS_DEADLINE.to_string(),
                },
                LaunchObservation::Retired => return,
            };
            svc.settle_launch_readiness(&attempt, evidence);
        });
    }
}
