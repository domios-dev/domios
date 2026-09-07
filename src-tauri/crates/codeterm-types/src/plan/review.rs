use super::types::AgentRef;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewVerdict {
    Accept,
    Bounce,
    Hold,
}

/// Evidence attached to the owning implementation task by a read-only review
/// pass. Never creates a task of its own; see docs/ORCHESTRATION.md review section.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReviewEvidence {
    pub id: String,
    pub from: AgentRef,
    pub verdict: ReviewVerdict,
    #[serde(default)]
    pub note: String,
    pub at: u64,
    #[serde(default)]
    pub changed_executable_surfaces: bool,
    #[serde(default)]
    pub distinct_oracle: bool,
}

/// A P0-exception authorization consumed on the next over-limit review round.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReviewOverride {
    pub owner: String,
    pub reason: String,
    pub reproduction: String,
    pub at: u64,
}

/// Immutable, append-only record that a review-round-limit exception was used.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReviewAuditEntry {
    pub id: String,
    pub owner: String,
    pub reason: String,
    pub reproduction: String,
    pub round: u32,
    pub at: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewCapacityClass {
    Review,
    CanonicalGate,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanEvidenceClass {
    Review,
    CanonicalQa,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanEvidenceReporterGrant {
    pub tab_id: String,
    pub session_id: String,
    pub session_binding_digest: String,
    pub class: PlanEvidenceClass,
    pub granted_by: AgentRef,
    pub granted_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReviewCapacityGrant {
    pub task_id: String,
    pub class: ReviewCapacityClass,
    pub granted_by: AgentRef,
    pub granted_at: u64,
}

/// Plan-level review-backpressure policy: bounded in_review WIP plus a
/// completion/closure credit ledger (entering review spends credit, closing
/// a review round earns it back).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReviewPolicy {
    pub wip_limit: u32,
    pub credit: i64,
    #[serde(default)]
    pub capacity_grants: Vec<ReviewCapacityGrant>,
}

impl Default for ReviewPolicy {
    fn default() -> Self {
        Self {
            wip_limit: DEFAULT_REVIEW_WIP_LIMIT,
            credit: 0,
            capacity_grants: Vec::new(),
        }
    }
}

pub const DEFAULT_REVIEW_WIP_LIMIT: u32 = 5;
pub const DEFAULT_MAX_REVIEW_ROUNDS: u32 = 2;
