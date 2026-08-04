use serde::{Deserialize, Serialize};

/// How long an open gate stays silent before it's considered stale (24 h).
pub const STALE_GATE_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GateStatus {
    Open,
    Answered,
    Escalated,
}

/// An immutable audit record of a gate answer (once written, never changed).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GateAnswer {
    pub text: String,
    pub by_tab_id: Option<String>,
    pub at: u64,
}

/// Structured decision request that parks blocking tasks until answered.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DecisionGate {
    pub id: String,
    pub question: String,
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub blocking_task_ids: Vec<String>,
    /// The tab the question was routed to (planner or orchestrator fallback).
    #[serde(default)]
    pub routed_to_tab_id: Option<String>,
    #[serde(default)]
    pub asked_by_tab_id: Option<String>,
    pub asked_at: u64,
    pub status: GateStatus,
    /// Immutable audit record; set once on first answer.
    #[serde(default)]
    pub answer: Option<GateAnswer>,
    #[serde(default)]
    pub escalated_at: Option<u64>,
    #[serde(default)]
    pub escalation_note: Option<String>,
}

impl DecisionGate {
    pub fn is_open(&self) -> bool {
        matches!(self.status, GateStatus::Open | GateStatus::Escalated)
    }

    pub fn is_stale(&self, now_ms: u64) -> bool {
        self.is_open() && now_ms.saturating_sub(self.asked_at) > STALE_GATE_MS
    }
}
