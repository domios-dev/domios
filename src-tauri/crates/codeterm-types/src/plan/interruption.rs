use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionIntent {
    #[default]
    Unspecified,
    ReadOnly,
    Mutating,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ProviderAllowance {
    pub remaining_pct: f32,
    pub observed_at: u64,
    pub stale_after_ms: u64,
    pub reserve_pct: f32,
}

impl ProviderAllowance {
    pub fn is_stale(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.observed_at) > self.stale_after_ms
    }

    pub fn permits(&self, intent: &ExecutionIntent, _now_ms: u64) -> bool {
        *intent != ExecutionIntent::Mutating
            || self.remaining_pct >= self.reserve_pct
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct InterruptionCheckpoint {
    pub task_id: String,
    pub attempt_id: String,
    pub tab_id: String,
    pub session_id: String,
    pub owner_epoch: u64,
    pub repo_path: String,
    pub worktree_path: String,
    pub base_head: String,
    pub current_head: String,
    pub git_status: String,
    pub diff_artifact: String,
    pub diff_sha256: String,
    pub completed_tests: Vec<String>,
    pub unresolved_work: String,
    pub next_acceptance_gate: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TaskInterruptionState {
    pub execution_intent: ExecutionIntent,
    pub allowance: Option<ProviderAllowance>,
    pub handoff_required: bool,
    pub checkpoint: Option<InterruptionCheckpoint>,
    pub owner_epoch: u64,
    pub owner_tab_id: Option<String>,
    pub owner_session_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AttemptInterruptionState {
    pub owner_epoch: u64,
    pub interrupted_at: Option<u64>,
    pub reason: Option<String>,
    pub checkpoint_digest: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AttemptContinuationState {
    pub epoch: u32,
    pub review_round: Option<u32>,
    pub state: Option<String>,
    pub milestone_id: Option<String>,
    pub delivered_at: Option<u64>,
}
