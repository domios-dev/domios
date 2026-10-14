use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};

use codeterm_types::decision::{
    DecisionAnswer, DecisionError, DecisionModel, DecisionQuestionsRequest, DecisionResponse,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

mod admission;
mod decision;
mod delivery_log;
#[cfg(test)]
mod replay_tests;
mod runtime;
mod snapshot;
#[cfg(test)]
mod snapshot_tests;
mod state;
#[cfg(test)]
mod tests;

pub(crate) use decision::criteria_version;
use decision::{bounded_questions, cadence, classify, error_reason, suppressed};
#[cfg(test)]
use decision::{questions, selected_learnings};
pub(crate) use delivery_log::{delivery_tick, DeliveryTick, TickFact};
pub(crate) use runtime::{journal, Supporter};
pub(crate) use snapshot::{
    actionable_pending, binding_digest, collect, collect_owner_messages, collect_worker_scope,
};
#[cfg(test)]
pub(crate) use state::Telemetry;
pub(crate) use state::{Claim, Gate, Observation};

pub(crate) const IDLE_NO_REPORT_SECS: u64 = 30 * 60;
const MIN_CONFIDENCE: f64 = 0.80;
const MAX_OBSERVATION_BYTES: usize = 32 * 1024;
const MAX_OBSERVATION_ROWS: usize = 32;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct Worker {
    pub task_id: String,
    pub tab_id: String,
    pub ack_received: bool,
    pub session_key: Option<String>,
    pub process: String,
    pub activity: String,
    pub appended_turns: Vec<String>,
    pub last_turn: Option<String>,
    pub turn_cursor: Option<String>,
    pub repeating: bool,
    pub recent_turn_hashes: Vec<String>,
    pub lane_commits: Vec<String>,
    pub prompt: Option<String>,
    pub safe_choice: Option<String>,
    pub safe_option: Option<usize>,
    pub elapsed_bucket: u64,
    pub idle_overdue: bool,
    pub held: bool,
    pub reported: bool,
    pub evidence_unavailable: bool,
}

impl Worker {
    fn unhealthy(&self) -> bool {
        !self.ack_received
            || self.session_key.is_none()
            || self.process != "present"
            || !matches!(self.activity.as_str(), "working" | "idle" | "waiting")
            || self.prompt.is_some()
            || self.evidence_unavailable
            || (!self.held && self.repeating)
            || (!self.held && !self.reported && self.idle_overdue)
    }

    fn heuristic_label(&self) -> &'static str {
        if self.prompt.is_some() {
            "waiting_on_prompt"
        } else if !self.held && !self.reported && self.idle_overdue {
            "idle_no_report"
        } else if !self.held && self.repeating {
            "looping"
        } else if self.unhealthy() {
            "other"
        } else {
            "on_track"
        }
    }

    /// The only Worker evidence that reaches the model or the identity hash:
    /// derived health, never session keys, turn ids or commit hashes.
    fn health(&self) -> Value {
        json!({
            "task": self.task_id, "tab": self.tab_id, "ack": self.ack_received,
            "bound": self.session_key.is_some(), "process": self.process, "activity": self.activity,
            "prompt": self.prompt, "idle": self.idle_overdue && !self.reported && !self.held,
            "looping": self.repeating && !self.held, "reported": self.reported,
            "held": self.held, "unknown": self.evidence_unavailable,
        })
    }

    fn next_step(&self, label: &str) -> String {
        let action = match label {
            "waiting_on_prompt" => match &self.safe_choice {
                Some(choice) => match self.safe_option {
                    Some(option) => format!("inspect the classified {:?} prompt, answer {choice} with codeterm tab respond {option} --tab {}", self.prompt, self.tab_id),
                    None => format!("inspect the classified {:?} prompt and select the declared safe choice {choice}", self.prompt),
                },
                None => "inspect the classified prompt and resolve its required decision".into(),
            },
            "idle_no_report" => format!("request the durable task report for {}", self.task_id),
            "looping" => format!("review progress on {} and choose a new approach", self.task_id),
            _ => self.open_liveness_check(),
        };
        format!("{label} on tab {}: {action}", self.tab_id)
    }

    /// ACK, session and process are separate checks. One that already holds
    /// is not asked again.
    fn missing_liveness(&self) -> Vec<&'static str> {
        let mut checks = Vec::new();
        if !self.ack_received {
            checks.push("ACK");
        }
        if self.session_key.is_none() {
            checks.push("bound session");
        }
        if self.process != "present" {
            checks.push("provider liveness");
        }
        checks
    }

    fn open_liveness_check(&self) -> String {
        let checks = self.missing_liveness();
        if checks.len() == 3 {
            return format!(
                "check the ACK, bound session and provider liveness for {}",
                self.task_id
            );
        }
        if checks.is_empty() {
            format!("review the remaining evidence for {}", self.task_id)
        } else {
            format!("check the {} for {}", checks.join(", "), self.task_id)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub(crate) enum HardWake {
    UndeliveredReport(String),
    NewReport(String),
    TaskState(String),
    TaskClosed(String),
    FailedGate(String),
    OwnerMessage(String),
    ChoiceAnswer(String),
    Incident(String),
    ModeChange,
}

impl HardWake {
    fn next_step(&self, plan: &str) -> String {
        match self {
            Self::TaskState(id) | Self::TaskClosed(id) => format!("inspect the changed task {id} with codeterm plan state {plan} and reconcile its review or dispatch"),
            Self::NewReport(id) => format!("read the new report {id} in {plan} and review its task"),
            Self::UndeliveredReport(id) => format!("retrieve persisted report {id} with codeterm orchestrator inspect --plan {plan} --json and review its task; investigate the undelivered report"),
            Self::FailedGate(id) => format!("inspect failed gate {id} in {plan} and assign its repair before advancing"),
            Self::OwnerMessage(id) => format!("read owner message {id} and apply its decision to {plan}"),
            Self::ChoiceAnswer(id) => format!("apply choice answer {id} and unblock its waiting work in {plan}"),
            Self::Incident(id) => format!("inspect incident {id} in {plan}; restore the exact worker binding or reassign its task"),
            Self::ModeChange => format!("reconcile task dispatch and review with the new mode of {plan}"),
        }
    }
}

/// Automatic plan reminder producers; each one needs its own Choice approval.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReminderKind {
    #[default]
    Owner,
    ModeChange,
    StalledTeam,
    CompleteWrapup,
    Worker,
}

impl ReminderKind {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::ModeChange => "mode_change",
            Self::StalledTeam => "stalled_team",
            Self::CompleteWrapup => "complete_wrapup",
            Self::Worker => "worker",
        }
    }
}

/// Observation identity beyond plan state. Bindings and adapters are digests,
/// so neither the model nor the journal sees a raw session nonce.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct Scope {
    pub kind: ReminderKind,
    pub role: String,
    pub workspace: String,
    pub team_id: Option<String>,
    pub tab_id: String,
    pub recipient_team: Option<String>,
    pub recipient_workspace: Option<String>,
    pub recipient_role: String,
    pub binding: String,
    pub task_id: Option<String>,
    pub attempt_id: Option<String>,
    pub lifecycle: String,
    pub adapter: String,
    pub criteria: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct Snapshot {
    pub plan_id: String,
    pub projection: BTreeMap<String, String>,
    pub delta: BTreeMap<String, String>,
    pub context: BTreeMap<String, Value>,
    pub workers: Vec<Worker>,
    pub hard: Vec<HardWake>,
    pub learnings: BTreeMap<String, String>,
    pub scope: Scope,
    pub truncated: bool,
}

impl Snapshot {
    pub(crate) fn hash(&self) -> String {
        let health: Vec<_> = self.workers.iter().map(Worker::health).collect();
        let state = json!({ "plan": self.plan_id, "projection": self.projection,
            "delta": self.delta, "hard": self.hard, "health": health, "learnings": self.learnings,
            "scope": self.scope, "truncated": self.truncated });
        crate::service::supporter::criteria_hash(&serde_json::to_vec(&state).unwrap_or_default())
    }

    /// Absolute state without the delivery-relative delta and hard events.
    fn state_hash(&self) -> String {
        let health: Vec<_> = self.workers.iter().map(Worker::health).collect();
        let state = json!({ "plan": self.plan_id, "projection": self.projection,
            "health": health, "learnings": self.learnings, "scope": self.scope,
            "truncated": self.truncated });
        crate::service::supporter::criteria_hash(&serde_json::to_vec(&state).unwrap_or_default())
    }

    fn healthy_and_unchanged(&self) -> bool {
        self.hard.is_empty()
            && self.delta.is_empty()
            && self.workers.iter().all(|worker| !worker.unhealthy())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Decision {
    pub wake: bool,
    pub tick_label: String,
    pub decided_by: String,
    pub reason: &'static str,
    pub observation_id: String,
    pub snapshot_hash: String,
    pub next_steps: Vec<String>,
    pub worker_labels: BTreeMap<String, String>,
    pub learning_ids: Option<Vec<String>>,
    pub scope: Scope,
    pub adapter: Option<String>,
    pub model: Option<String>,
}

impl Decision {
    fn soft_class(&self) -> String {
        json!({ "tick": self.tick_label, "workers": self.worker_labels }).to_string()
    }

    /// Bounded diagnostics only: reason enums, hashes, scoped ids and adapter
    /// metadata; never the observation, next-step text or a binding.
    pub(crate) fn journal_event(&self, tab: &str, plan: &str, now: u64) -> Value {
        json!({ "time": now, "kind": crate::service::supporter::VerdictKind::Heartbeat,
            "tab": tab, "plan_id": plan, "label": self.tick_label,
            "action": if self.wake { "wake" } else { "suppress" }, "reason": self.reason,
            "decided_by": self.decided_by, "observation_id": self.observation_id,
            "state_hash": self.snapshot_hash, "reminder": self.scope.kind,
            "task_id": self.scope.task_id, "attempt_id": self.scope.attempt_id,
            "adapter": self.adapter, "model_id": self.model, "criteria_version": self.scope.criteria,
            "tick_class": self.tick_label, "worker_classes": self.worker_labels,
            "learning_ids": self.learning_ids, "asked": null })
    }

    pub(crate) fn next_step_text(&self) -> String {
        format!("Next step:\n{}", self.next_steps.join("\n"))
    }
}
