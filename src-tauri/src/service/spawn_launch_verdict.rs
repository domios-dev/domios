//! A spawn answers for the launch it started: an agent whose provider exits
//! before readiness is a failed spawn, never a success with a dead tab behind it.

use super::boot_stage::BootStage;
use super::ServiceContext;
use crate::multiplexer::types::Tab;
use std::time::{Duration, Instant};

#[cfg(test)]
#[path = "spawn_launch_verdict/tests.rs"]
mod tests;

/// Bounded below the 30 s forward deadline of a relayed or loopback-HTTP caller,
/// so a live launch still in progress is never reported as a transport failure.
pub(crate) const SPAWN_LAUNCH_VERDICT_DEADLINE: Duration = Duration::from_secs(20);
const OUTPUT_TAIL_LINES: usize = 20;
const OUTPUT_TAIL_BYTES: usize = 2048;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SpawnLaunchVerdict {
    Ready,
    /// The provider is alive but not ready yet: an owner prompt, a provisional
    /// stage, or a launch still in progress at the deadline. Never a success claim.
    Starting {
        stage: Option<BootStage>,
    },
    Failed {
        reason: String,
    },
}

impl SpawnLaunchVerdict {
    pub(crate) fn launch_state(&self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Starting { .. } => "starting",
            Self::Failed { .. } => "failed",
        }
    }

    pub(crate) fn warning(&self, tab_id: &str) -> Option<String> {
        let Self::Starting { stage } = self else {
            return None;
        };
        let stage = stage.map(|stage| stage.as_str()).unwrap_or("launching");
        Some(format!(
            "agent in Tab {tab_id} has not reached readiness yet (stage: {stage}); check `codeterm tab list` before sending work"
        ))
    }

    /// The spawn's own warning followed by this verdict's, as one response field.
    pub(crate) fn merged_warning(
        &self,
        tab_id: &str,
        spawn_warning: Option<&str>,
    ) -> Option<String> {
        let warnings: Vec<String> = spawn_warning
            .map(str::to_string)
            .into_iter()
            .chain(self.warning(tab_id))
            .collect();
        (!warnings.is_empty()).then(|| warnings.join("; "))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SpawnLaunchFailure {
    pub tab_id: String,
    pub reason: String,
    pub output_tail: Option<String>,
}

impl SpawnLaunchFailure {
    pub(crate) const CODE: &'static str = "launch_failed";

    pub(crate) fn message(&self) -> String {
        let mut message = format!(
            "agent launch failed in Tab {}: {}",
            self.tab_id, self.reason
        );
        if let Some(tail) = self.output_tail.as_deref() {
            message.push_str("\nprovider output (tail):\n");
            message.push_str(tail);
        }
        message
    }

    pub(crate) fn data(&self) -> serde_json::Value {
        serde_json::json!({
            "type": Self::CODE,
            "tab_id": self.tab_id,
            "reason": self.reason,
            "output_tail": self.output_tail,
            "retry_hint": "the provider exited before readiness; the same launch fails again — fix what output_tail names, then close the failed Tab",
        })
    }
}

/// The verdict a tab already carries for its launch, or `None` while it is
/// still on its way to one.
pub(crate) fn classify_spawn_launch(tab: Option<&Tab>) -> Option<SpawnLaunchVerdict> {
    let Some(tab) = tab else {
        return Some(SpawnLaunchVerdict::Failed {
            reason: "the Tab was closed before its agent reached readiness".into(),
        });
    };
    match tab.boot_stage {
        Some(BootStage::SpawnFailed) => Some(SpawnLaunchVerdict::Failed {
            reason: tab
                .boot_failure_reason
                .clone()
                .unwrap_or_else(|| "agent launch failed".into()),
        }),
        Some(BootStage::AgentReady | BootStage::PtyReattached) => Some(SpawnLaunchVerdict::Ready),
        Some(
            stage @ (BootStage::AwaitingOwnerPrompt
            | BootStage::LaunchDeadlineElapsed
            | BootStage::LaunchBindingUnconfirmed),
        ) => Some(SpawnLaunchVerdict::Starting { stage: Some(stage) }),
        None if !tab.agent_booting => Some(SpawnLaunchVerdict::Ready),
        _ => None,
    }
}

/// The last lines a failed launch printed, bounded and secret-redacted.
pub(crate) fn output_tail(screen: &str) -> Option<String> {
    let cleaned = crate::pty::ansi::clean_terminal_output(screen);
    let lines: Vec<&str> = cleaned.lines().map(str::trim_end).collect();
    let end = lines.iter().rposition(|line| !line.is_empty())? + 1;
    let start = end.saturating_sub(OUTPUT_TAIL_LINES);
    let tail = lines[start..end].join("\n");
    let tail = super::launch_diagnostics::tail_bytes(&tail, OUTPUT_TAIL_BYTES);
    Some(super::launch_diagnostics::redact_secrets(&tail))
}

impl ServiceContext {
    /// Wait on boot-stage transitions for this tab's launch verdict. The
    /// deadline bounds only the wait; reaching it reports `Starting`, never ready.
    pub(crate) fn await_spawn_launch_verdict(
        &self,
        tab_id: &str,
        deadline: Duration,
    ) -> Result<SpawnLaunchVerdict, SpawnLaunchFailure> {
        let mut verdict = None;
        super::boot_wait::wait_until(Instant::now() + deadline, || {
            verdict = classify_spawn_launch(self.mux.read().get_tab(tab_id));
            verdict.is_some()
        });
        match verdict {
            Some(SpawnLaunchVerdict::Failed { reason }) => {
                log::error!("[spawn-verdict] Tab {tab_id} launch failed: {reason}");
                Err(SpawnLaunchFailure {
                    tab_id: tab_id.to_string(),
                    reason,
                    output_tail: self.launch_output_tail(tab_id),
                })
            }
            Some(verdict) => Ok(verdict),
            None => Ok(SpawnLaunchVerdict::Starting {
                stage: self
                    .mux
                    .read()
                    .get_tab(tab_id)
                    .and_then(|tab| tab.boot_stage),
            }),
        }
    }

    pub(crate) fn launch_output_tail(&self, tab_id: &str) -> Option<String> {
        let pty_id = self.mux.read().get_tab(tab_id)?.pty_id.clone()?;
        let screen = self.screen_state.lock().get_screen_text(&pty_id)?;
        output_tail(&screen)
    }
}
