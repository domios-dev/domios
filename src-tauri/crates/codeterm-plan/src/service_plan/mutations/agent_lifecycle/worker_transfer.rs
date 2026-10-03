use super::*;
use crate::plan::{PlanMode, PlanStatus, TaskStatus};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedWorkerTransferBinding {
    pub plan_id: String,
    pub team_id: String,
    pub owner_tab_id: String,
    pub owner_session_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub worker_tab_id: String,
    pub workspace_id: String,
    pub worktree_path: String,
    pub session_id: String,
    pub binding_nonce: String,
    pub provider: String,
    pub owner_epoch: u64,
}

impl PlanStore {
    pub fn with_verified_worker_transfer_binding<F>(
        &self,
        expected: &VerifiedWorkerTransferBinding,
        commit_staged_mux: F,
    ) -> Result<(), PlanError>
    where
        F: FnOnce(),
    {
        let _writer_turn = self.mutation_writer.enter();
        let plans = self.list_full();
        let mut assignments = 0;
        let mut lanes = 0;
        let mut worktrees = 0;
        let mut open_attempts = 0;
        let mut matched = None;

        for plan in &plans {
            for task in &plan.tasks {
                if !task.status.is_terminal() {
                    assignments += usize::from(
                        task.assigned_tab_id.as_deref() == Some(expected.worker_tab_id.as_str()),
                    );
                    lanes += usize::from(
                        task.lane.tab_id.as_deref() == Some(expected.worker_tab_id.as_str()),
                    );
                    worktrees += usize::from(
                        task.lane.worktree_path.as_deref() == Some(expected.worktree_path.as_str()),
                    );
                }
                for attempt in &task.execution_attempts {
                    if attempt.executor_tab_id == expected.worker_tab_id
                        && attempt.closed_at.is_none()
                        && attempt.retired_at.is_none()
                    {
                        open_attempts += 1;
                    }
                    if plan.id == expected.plan_id
                        && task.id == expected.task_id
                        && attempt.id == expected.attempt_id
                    {
                        matched = Some((plan, task, attempt));
                    }
                }
            }
        }

        if assignments != 1 || lanes != 1 || worktrees != 1 || open_attempts != 1 {
            return Err(refused(
                "worker assignment, lane, or open attempt is not unique",
            ));
        }

        let Some((plan, task, attempt)) = matched else {
            return Err(refused("the verified task attempt no longer exists"));
        };
        if plan.status != PlanStatus::Active
            || !matches!(&plan.mode, PlanMode::Approved | PlanMode::Execution)
            || plan.team_id.as_deref() != Some(expected.team_id.as_str())
            || plan.owner.tab_id.as_deref() != Some(expected.owner_tab_id.as_str())
            || plan
                .owner
                .session_id
                .as_deref()
                .is_some_and(|session_id| session_id != expected.owner_session_id.as_str())
            || task.assigned_tab_id.as_deref() != Some(expected.worker_tab_id.as_str())
            || task.lane.tab_id.as_deref() != Some(expected.worker_tab_id.as_str())
            || task.lane.workspace_id.as_deref() != Some(expected.workspace_id.as_str())
            || task.lane.worktree_path.as_deref() != Some(expected.worktree_path.as_str())
            || !matches!(&task.status, TaskStatus::InProgress | TaskStatus::InReview)
            || task.interruption.handoff_required
            || task.interruption.checkpoint.is_some()
            || task.interruption.owner_tab_id.as_deref() != Some(expected.worker_tab_id.as_str())
            || task.interruption.owner_session_id.as_deref() != Some(expected.session_id.as_str())
            || task.interruption.owner_epoch != expected.owner_epoch
            || attempt.executor_tab_id != expected.worker_tab_id
            || attempt.session_id.as_deref() != Some(expected.session_id.as_str())
            || attempt.session_binding_nonce.as_deref() != Some(expected.binding_nonce.as_str())
            || attempt.provider.as_deref() != Some(expected.provider.as_str())
            || attempt.agent_type.as_deref() != Some(expected.provider.as_str())
            || attempt.acknowledged_at.is_none()
            || attempt.closed_at.is_some()
            || attempt.retired_at.is_some()
            || attempt.interruption.owner_epoch != expected.owner_epoch
            || !attempt.placement.is_isolated_lane()
            || attempt.mesh_peer.is_some()
            || attempt.remote_dispatch_state.is_some()
            || attempt.remote_worktree_path.is_some()
            || attempt.remote_lane_tip.is_some()
            || attempt.remote_merge_commit.is_some()
            || attempt.worktree_path.as_deref() != task.lane.worktree_path.as_deref()
            || attempt.branch.as_deref() != task.lane.branch.as_deref()
            || attempt.base.as_deref() != task.lane.base.as_deref()
            || task.lane.worktree_path.as_deref().is_none_or(str::is_empty)
            || task.lane.branch.as_deref().is_none_or(str::is_empty)
        {
            return Err(refused(
                "the task attempt no longer matches its verified owner",
            ));
        }

        commit_staged_mux();
        Ok(())
    }
}

fn refused(reason: &str) -> PlanError {
    PlanError::Io(format!("worker transfer refused: {reason}"))
}
