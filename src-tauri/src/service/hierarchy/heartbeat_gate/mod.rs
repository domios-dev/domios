use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use codeterm_types::decision::{
    DecisionAnswer, DecisionError, DecisionModel, DecisionQuestionsRequest, DecisionResponse,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

mod decision;
#[cfg(test)]
mod replay_tests;
mod runtime;
mod snapshot;
#[cfg(test)]
mod snapshot_tests;
mod state;
#[cfg(test)]
mod tests;

use decision::{classify, fallback};
#[cfg(test)]
use decision::{questions, selected_learnings};
pub(crate) use runtime::decide;
pub(crate) use snapshot::collect;
pub(crate) use state::{Gate, Observation};

pub(crate) const SAFETY_STATUS_SECS: u64 = 2 * 60 * 60;
pub(crate) const IDLE_NO_REPORT_SECS: u64 = 30 * 60;
const MIN_CONFIDENCE: f64 = 0.80;

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
            _ => format!("check the ACK, bound session and provider liveness for {}", self.task_id),
        };
        format!("{label} on tab {}: {action}", self.tab_id)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub(crate) enum HardWake {
    UndeliveredReport(String),
    NewReport(String),
    TaskState(String),
    FailedGate(String),
    OwnerMessage(String),
    ChoiceAnswer(String),
    Incident(String),
    ModeChange,
}

impl HardWake {
    fn next_step(&self, plan: &str) -> String {
        match self {
            Self::TaskState(id) => format!("inspect the changed task {id} with codeterm plan state {plan} and reconcile its review or dispatch"),
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
}

impl Snapshot {
    fn hash(&self) -> String {
        let health: Vec<_> = self.workers.iter().map(|worker| json!({
            "task": worker.task_id, "tab": worker.tab_id, "ack": worker.ack_received,
            "bound": worker.session_key.is_some(), "process": worker.process, "activity": worker.activity,
            "prompt": worker.prompt, "idle": worker.idle_overdue && !worker.reported && !worker.held,
            "looping": worker.repeating && !worker.held, "reported": worker.reported,
            "held": worker.held, "unknown": worker.evidence_unavailable,
        })).collect();
        let state = json!({ "plan": self.plan_id, "projection": self.projection,
            "delta": self.delta, "hard": self.hard, "health": health, "learnings": self.learnings });
        crate::service::supporter::criteria_hash(&serde_json::to_vec(&state).unwrap_or_default())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Decision {
    pub wake: bool,
    pub safety: bool,
    pub tick_label: String,
    pub decided_by: String,
    pub reason: &'static str,
    pub observation_id: String,
    pub snapshot_hash: String,
    pub next_steps: Vec<String>,
    pub worker_labels: BTreeMap<String, String>,
    pub learning_ids: Option<Vec<String>>,
}

impl Decision {
    fn soft_class(&self) -> String {
        json!({ "tick": self.tick_label, "workers": self.worker_labels,
            "uncertain": self.reason == "low_confidence" })
        .to_string()
    }

    pub(crate) fn journal_event(&self, tab: &str, plan: &str, now: u64) -> Value {
        json!({ "time": now, "kind": crate::service::supporter::VerdictKind::Heartbeat,
            "tab": tab, "plan_id": plan, "label": if self.safety { "status_only" } else if self.wake { "action_needed" } else { "nothing_changed" },
            "action": if self.wake { "wake" } else { "suppress" }, "reason": self.reason,
            "decided_by": self.decided_by, "observation_id": self.observation_id,
            "state_hash": self.snapshot_hash, "safety": self.safety,
            "tick_class": self.tick_label, "worker_classes": self.worker_labels,
            "next_steps": self.next_steps, "learning_ids": self.learning_ids, "asked": null })
    }

    pub(crate) fn observation_event(
        &self,
        snapshot: &Snapshot,
        tab: &str,
        plan: &str,
        now: u64,
    ) -> Value {
        let mut event = self.journal_event(tab, plan, now);
        event["observation"] = serde_json::to_value(snapshot).unwrap_or(Value::Null);
        event
    }

    pub(crate) fn next_step_text(&self) -> String {
        if self.safety {
            "Heartbeat safety status: scheduler is active. Next step: inspect the plan when useful; no new incident was observed.".into()
        } else {
            format!("Next step:\n{}", self.next_steps.join("\n"))
        }
    }
}
