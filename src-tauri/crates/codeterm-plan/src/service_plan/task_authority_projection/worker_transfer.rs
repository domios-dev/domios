use crate::plan::{PlanMode, PlanStatus, TaskStatus, WorkerPlacement};
use crate::service_plan::mutations::agent_lifecycle::VerifiedWorkerTransferBinding;

#[derive(serde::Serialize)]
pub(in crate::service_plan) struct WorkerTransferRead {
    pub assignments: u64,
    pub lanes: u64,
    pub worktrees: u64,
    pub open_attempts: u64,
    pub subject: Option<WorkerTransferSubjectRead>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub(in crate::service_plan) struct WorkerTransferSubjectRead {
    pub plan_status: PlanStatus,
    pub plan_mode: PlanMode,
    pub team_id: Option<String>,
    pub plan_owner_tab: Option<String>,
    pub plan_owner_session: Option<String>,
    pub task_status: TaskStatus,
    pub assigned_tab: Option<String>,
    pub lane_tab: Option<String>,
    pub workspace_id: Option<String>,
    pub lane_path: Option<String>,
    pub lane_branch: Option<String>,
    pub lane_base: Option<String>,
    pub handoff_required: bool,
    pub has_checkpoint: bool,
    pub task_owner_tab: Option<String>,
    pub task_owner_session: Option<String>,
    pub task_owner_epoch: u64,
    pub executor_tab: String,
    pub session_id: Option<String>,
    pub nonce: Option<String>,
    pub provider: Option<String>,
    pub agent_type: Option<String>,
    pub acknowledged_at: Option<u64>,
    pub closed_at: Option<u64>,
    pub retired_at: Option<u64>,
    pub attempt_owner_epoch: u64,
    pub placement: WorkerPlacement,
    pub has_remote_metadata: bool,
    pub attempt_path: Option<String>,
    pub attempt_branch: Option<String>,
    pub attempt_base: Option<String>,
}

impl WorkerTransferSubjectRead {
    pub fn matches(&self, expected: &VerifiedWorkerTransferBinding) -> bool {
        self.plan_status == PlanStatus::Active
            && matches!(self.plan_mode, PlanMode::Approved | PlanMode::Execution)
            && self.team_id.as_deref() == Some(expected.team_id.as_str())
            && self.plan_owner_tab.as_deref() == Some(expected.owner_tab_id.as_str())
            && self
                .plan_owner_session
                .as_deref()
                .is_none_or(|session| session == expected.owner_session_id)
            && self.assigned_tab.as_deref() == Some(expected.worker_tab_id.as_str())
            && self.lane_tab.as_deref() == Some(expected.worker_tab_id.as_str())
            && self.workspace_id.as_deref() == Some(expected.workspace_id.as_str())
            && self.lane_path.as_deref() == Some(expected.worktree_path.as_str())
            && matches!(
                self.task_status,
                TaskStatus::InProgress | TaskStatus::InReview
            )
            && !self.handoff_required
            && !self.has_checkpoint
            && self.task_owner_tab.as_deref() == Some(expected.worker_tab_id.as_str())
            && self.task_owner_session.as_deref() == Some(expected.session_id.as_str())
            && self.task_owner_epoch == expected.owner_epoch
            && self.executor_tab == expected.worker_tab_id
            && self.session_id.as_deref() == Some(expected.session_id.as_str())
            && self.nonce.as_deref() == Some(expected.binding_nonce.as_str())
            && self.provider.as_deref() == Some(expected.provider.as_str())
            && self.agent_type.as_deref() == Some(expected.provider.as_str())
            && self.acknowledged_at.is_some()
            && self.closed_at.is_none()
            && self.retired_at.is_none()
            && self.attempt_owner_epoch == expected.owner_epoch
            && self.placement.is_isolated_lane()
            && !self.has_remote_metadata
            && self.attempt_path == self.lane_path
            && self.attempt_branch == self.lane_branch
            && self.attempt_base == self.lane_base
            && self
                .lane_path
                .as_deref()
                .is_some_and(|path| !path.is_empty())
            && self
                .lane_branch
                .as_deref()
                .is_some_and(|branch| !branch.is_empty())
    }
}
