use super::types::AttachmentRef;
use super::{AttemptContinuationState, AttemptInterruptionState};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TaskLaneMetadata {
    #[serde(default)]
    pub tab_id: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub workspace_name: Option<String>,
    #[serde(default)]
    pub worktree_path: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub destination: Option<String>,
    #[serde(default)]
    pub cleanup_status: Option<String>,
    #[serde(default)]
    pub cleanup_hints: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TaskCompetitionMetadata {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub attempts: Option<u32>,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub judge_hint: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TaskExecutionReportSnapshot {
    pub id: String,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reporter_tab_id: Option<String>,
    #[serde(default)]
    pub changed_files: Vec<String>,
    #[serde(default)]
    pub test_cmd: Option<String>,
    #[serde(default)]
    pub test_result: Option<String>,
    #[serde(default)]
    pub skipped: Option<String>,
    #[serde(default)]
    pub attachments: Vec<AttachmentRef>,
    #[serde(default)]
    pub plan_skill_manifest: Vec<PlanSkillManifestEntry>,
    #[serde(default)]
    pub loaded_plan_skills: Vec<LoadedPlanSkillEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanSkillManifestEntry {
    pub slug: String,
    pub title: String,
    pub purpose: String,
    pub content_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoadedPlanSkillEvidence {
    pub slug: String,
    pub content_digest: String,
    pub loaded_at: u64,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkerPlacement {
    #[default]
    IsolatedLane,
    CanonicalWriter,
}

impl WorkerPlacement {
    pub fn is_isolated_lane(&self) -> bool {
        matches!(self, Self::IsolatedLane)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TaskExecutionAttempt {
    pub id: String,
    pub executor_tab_id: String,
    #[serde(default)]
    pub executor_title: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub agent_type: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    #[serde(default)]
    pub session_model: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub session_binding_nonce: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub parent_tab_id: Option<String>,
    #[serde(default)]
    pub orchestrator_tab_id: Option<String>,
    #[serde(default)]
    pub mesh_peer: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub worktree_path: Option<String>,
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub destination: Option<String>,
    /// `Some` means this attempt came from the gate-aware dispatch path. An
    /// empty list is deliberate: that dispatch granted no gate permission.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorized_gates: Option<Vec<String>>,
    /// Set only by an explicit dispatch opt-in; never derived from a checkout path.
    #[serde(default, skip_serializing_if = "WorkerPlacement::is_isolated_lane")]
    pub placement: WorkerPlacement,
    #[serde(default)]
    pub report: Option<TaskExecutionReportSnapshot>,
    #[serde(default)]
    pub assigned_at: Option<u64>,
    #[serde(default)]
    pub acknowledged_at: Option<u64>,
    #[serde(default)]
    pub continuation: AttemptContinuationState,
    #[serde(default)]
    pub interruption: AttemptInterruptionState,
    #[serde(default)]
    pub reported_at: Option<u64>,
    #[serde(default)]
    pub reviewed_at: Option<u64>,
    #[serde(default)]
    pub closed_at: Option<u64>,
    #[serde(default)]
    pub closure_reason: Option<String>,
    // Fence, not bookkeeping: a retired attempt can never re-enter startup, so a
    // late dispatch_ready cannot resurrect ownership after its tab is gone.
    #[serde(default)]
    pub retired_at: Option<u64>,
    #[serde(default)]
    pub competition_group_id: Option<String>,
    #[serde(default)]
    pub competition_lane: Option<u32>,
    #[serde(default)]
    pub judge_verdict: Option<String>,
    #[serde(default)]
    pub judge_reason: Option<String>,
    #[serde(default)]
    pub judged_at: Option<u64>,
    #[serde(default)]
    pub winner: bool,
    #[serde(default)]
    pub plan_skill_manifest: Vec<PlanSkillManifestEntry>,
    #[serde(default)]
    pub loaded_plan_skills: Vec<LoadedPlanSkillEvidence>,
}
