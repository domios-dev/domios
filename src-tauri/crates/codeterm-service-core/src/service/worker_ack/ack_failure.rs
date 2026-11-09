use super::*;
use crate::service::agent_runtime::full_access::FULL_ACCESS_UNAVAILABLE;
use codeterm_types::agent::chat_types::ChatMsg;

pub const WORKER_ACK_FAILED: &str = "worker_ack_failed";
const NO_OUTPUT: &str = "the acknowledgement command gave no output";

/// A settled tool row that ran this tab's own `agent ack`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct AckToolOutcome {
    pub failed: bool,
    pub cause: String,
}

/// The latest `agent ack` output of a tab's attempt; `reported` once a failure reached the parent.
#[derive(Debug, Clone)]
pub(super) struct AckEvidence {
    attempt_id: String,
    cause: String,
    reported: bool,
}

fn runs_agent_ack(command: &str) -> bool {
    let words = command
        .split(|c: char| c.is_whitespace() || "\"'`,[]()".contains(c))
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    let cli = words.iter().position(|word| {
        let name = word.rsplit(['/', '\\']).next().unwrap_or(word);
        let name = name.strip_suffix(".exe").unwrap_or(name);
        name == "domios" || name == "codeterm"
    });
    cli.is_some_and(|cli| words[cli..].windows(2).any(|pair| pair == ["agent", "ack"]))
}

/// What a tool row says about the tab's acknowledgement. Only the host sees this when the agent's
/// sandbox blocks the CLI from reaching the daemon, so it is the cause the parent gets.
pub(crate) fn ack_tool_outcome(item: &ChatMsg) -> Option<AckToolOutcome> {
    if item.kind != "tool_call" {
        return None;
    }
    let failed = match item.tool_status.as_deref() {
        Some("failed") => true,
        Some("completed") => item.tool_error == Some(true),
        _ => return None,
    };
    let host_tool = item
        .tool_name
        .as_deref()
        .is_some_and(|name| name.contains("agent_startup_ack"));
    if !host_tool
        && ![item.tool_args.as_deref(), item.tool_title.as_deref()]
            .into_iter()
            .flatten()
            .any(runs_agent_ack)
    {
        return None;
    }
    let output = item.tool_result.as_deref().unwrap_or_default();
    let cause = compact_provider_failure(output);
    Some(AckToolOutcome {
        failed,
        cause: if cause.is_empty() {
            NO_OUTPUT.to_owned()
        } else {
            cause
        },
    })
}

impl WorkerAckRegistry {
    /// Keeps the latest output per tab; true only for an attempt's first failure, which the parent hears.
    pub(crate) fn record_ack_outcome(
        &self,
        tab_id: &str,
        attempt_id: &str,
        outcome: &AckToolOutcome,
    ) -> bool {
        let mut evidence = self.ack_evidence.lock();
        let reported = evidence
            .get(tab_id)
            .is_some_and(|prior| prior.attempt_id == attempt_id && prior.reported);
        evidence.insert(
            tab_id.to_owned(),
            AckEvidence {
                attempt_id: attempt_id.to_owned(),
                cause: outcome.cause.clone(),
                reported: reported || outcome.failed,
            },
        );
        outcome.failed && !reported
    }

    /// A row that settled without the daemon recording an ACK still names the cause at the deadline.
    pub(crate) fn ack_cause(&self, tab_id: &str, attempt_id: &str) -> Option<String> {
        self.ack_evidence
            .lock()
            .get(tab_id)
            .filter(|evidence| evidence.attempt_id == attempt_id)
            .map(|evidence| evidence.cause.clone())
    }

    pub(crate) fn reported_ack_failure(&self, tab_id: &str) -> Option<String> {
        self.ack_evidence
            .lock()
            .get(tab_id)
            .filter(|evidence| evidence.reported)
            .map(|evidence| evidence.cause.clone())
    }

    pub(crate) fn clear_ack_evidence(&self, tab_id: &str) {
        self.ack_evidence.lock().remove(tab_id);
    }
}

impl ServiceContext {
    /// The one live attempt this tab still owes an acknowledgement, with who it reports to.
    fn unacknowledged_attempt(
        &self,
        tab_id: &str,
    ) -> Option<(crate::service::plan::LiveTabAttempt, String)> {
        let attempts = self
            .plan_store
            .live_task_attempts_for_tab(tab_id)
            .map_err(|error| {
                log::warn!("[worker-ack] attempt read failed tab={tab_id}: {error}");
            })
            .ok()?;
        let mut unacknowledged = attempts.into_iter().filter(|candidate| {
            candidate.plan_active
                && matches!(
                    candidate.task_status,
                    crate::plan::TaskStatus::Pending | crate::plan::TaskStatus::InProgress
                )
                && candidate.attempt.acknowledged_at.is_none()
                && candidate.attempt.retired_at.is_none()
        });
        let candidate = unacknowledged.next()?;
        if unacknowledged.next().is_some() {
            return None;
        }
        let parent = candidate
            .attempt
            .orchestrator_tab_id
            .clone()
            .or_else(|| candidate.attempt.parent_tab_id.clone())
            .or_else(|| {
                self.mux
                    .read()
                    .get_tab(tab_id)
                    .and_then(|tab| tab.parent_tab_id.clone())
            })?;
        Some((candidate, parent))
    }

    fn tab_title(&self, tab_id: &str) -> String {
        self.mux
            .read()
            .get_tab(tab_id)
            .map_or_else(|| "Worker".into(), |tab| tab.title.clone())
    }

    /// Records a settled `agent ack` tool row of a tab that still owes its ACK; a failed one reaches
    /// the parent at once with its cause, ahead of the startup deadline.
    pub(crate) fn record_worker_ack_tool_outcome(&self, tab_id: &str, outcome: &AckToolOutcome) {
        let Some((candidate, parent)) = self.unacknowledged_attempt(tab_id) else {
            return;
        };
        let attempt_id = &candidate.attempt.id;
        if !self
            .worker_ack
            .record_ack_outcome(tab_id, attempt_id, outcome)
        {
            return;
        }
        log::error!(
            "[worker-ack] {WORKER_ACK_FAILED} task={} attempt={attempt_id} tab={tab_id}: {}",
            candidate.task_id,
            outcome.cause
        );
        self.publish_worker_lifecycle_report(
            &parent,
            tab_id,
            &self.tab_title(tab_id),
            &candidate.plan_id,
            &candidate.task_id,
            json!({
                "kind": WORKER_ACK_FAILED,
                "plan_id": candidate.plan_id,
                "task_id": candidate.task_id,
                "tab_id": tab_id,
                "attempt_id": attempt_id,
                "failure_reason": outcome.cause,
            }),
        );
    }

    /// The parent of a tab whose declared full access did not take hears why at launch.
    pub(crate) fn report_full_access_unavailable(&self, tab_id: &str, reason: &str) {
        if let Some((candidate, parent)) = self.unacknowledged_attempt(tab_id) {
            self.publish_worker_lifecycle_report(
                &parent,
                tab_id,
                &self.tab_title(tab_id),
                &candidate.plan_id,
                &candidate.task_id,
                json!({
                    "kind": FULL_ACCESS_UNAVAILABLE,
                    "task_id": candidate.task_id,
                    "tab_id": tab_id,
                    "attempt_id": candidate.attempt.id,
                    "failure_reason": reason,
                }),
            );
            return;
        }
        let parent = self
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.parent_tab_id.clone());
        if let Some(parent) = parent {
            self.publish_agent_startup_report(
                &parent,
                tab_id,
                &self.tab_title(tab_id),
                json!({"kind": FULL_ACCESS_UNAVAILABLE, "failure_reason": reason}),
            );
        }
    }

    /// Why this attempt never acknowledged, when the host saw it: its own `agent ack` output, or a
    /// full-access mode the launch declared and the session never reached.
    pub(super) fn worker_ack_failure_cause(
        &self,
        tab_id: &str,
        attempt_id: &str,
    ) -> Option<String> {
        self.worker_ack.ack_cause(tab_id, attempt_id).or_else(|| {
            self.mux
                .read()
                .get_tab(tab_id)?
                .agent_runtime_state
                .as_ref()?
                .get("accessFailure")?
                .as_str()
                .map(str::to_owned)
        })
    }
}

#[cfg(test)]
#[path = "ack_failure/tests.rs"]
mod tests;
