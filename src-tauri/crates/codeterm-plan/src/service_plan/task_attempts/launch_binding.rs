use super::super::{PlanError, PlanStore};
use crate::plan::{PlanStatus, TaskExecutionAttempt, TaskStatus};

#[derive(Debug, Clone)]
pub struct CompleteAcknowledgedDispatchBinding {
    pub plan_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub tab_id: String,
    pub team_id: String,
    pub creator_tab_id: String,
    pub provider: String,
    pub session_id: String,
    pub nonce: String,
    pub owner_epoch: u64,
}

impl PlanStore {
    pub fn complete_acknowledged_dispatch_binding(
        &self,
        request: CompleteAcknowledgedDispatchBinding,
        now_ms: u64,
    ) -> Result<TaskExecutionAttempt, PlanError> {
        let plan_id = request.plan_id.clone();
        let mut completed = None;
        self.with_plan_if_changed(&plan_id, now_ms, |plan| {
            if plan.status != PlanStatus::Active
                || plan.team_id.as_deref() != Some(request.team_id.as_str())
                || self
                    .active_plan_for_team(&request.team_id)
                    .map(|(id, _)| id)
                    != Some(plan_id.clone())
                || request.owner_epoch == 0
                || [
                    &request.plan_id,
                    &request.task_id,
                    &request.attempt_id,
                    &request.tab_id,
                    &request.team_id,
                    &request.creator_tab_id,
                    &request.provider,
                    &request.session_id,
                    &request.nonce,
                ]
                .iter()
                .any(|value| value.trim().is_empty())
            {
                return Err(refused());
            }
            let mut owners = Vec::new();
            for active in self
                .list_full()
                .into_iter()
                .filter(|plan| plan.status == PlanStatus::Active)
            {
                for task in active
                    .tasks
                    .iter()
                    .filter(|task| !task.status.is_terminal())
                {
                    let open = task
                        .execution_attempts
                        .iter()
                        .filter(|attempt| {
                            attempt.executor_tab_id == request.tab_id
                                && attempt.closed_at.is_none()
                                && attempt.retired_at.is_none()
                        })
                        .collect::<Vec<_>>();
                    if task.assigned_tab_id.as_deref() == Some(request.tab_id.as_str())
                        && open.is_empty()
                    {
                        return Err(refused());
                    }
                    owners.extend(
                        open.into_iter().map(|attempt| {
                            (active.id.clone(), task.id.clone(), attempt.id.clone())
                        }),
                    );
                }
            }
            if owners
                != vec![(
                    request.plan_id.clone(),
                    request.task_id.clone(),
                    request.attempt_id.clone(),
                )]
            {
                return Err(refused());
            }
            let manifest = super::super::plan_skills::plan_skill_manifest_for_plan(plan)?;
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == request.task_id)
                .ok_or(PlanError::TaskNotFound)?;
            if task.status != TaskStatus::InProgress
                || task.assigned_tab_id.as_deref() != Some(request.tab_id.as_str())
                || task.lane.tab_id.as_deref() != Some(request.tab_id.as_str())
                || task.peer.is_some()
                || task.competition.is_some()
                || task.interruption.owner_epoch != request.owner_epoch
                || task.interruption.owner_tab_id.as_deref() != Some(request.tab_id.as_str())
                || task
                    .interruption
                    .owner_session_id
                    .as_deref()
                    .is_some_and(|id| id != request.session_id)
                || task.interruption.handoff_required
                || task.interruption.checkpoint.is_some()
                || task
                    .execution_attempts
                    .iter()
                    .filter(|attempt| attempt.closed_at.is_none() && attempt.retired_at.is_none())
                    .count()
                    != 1
                || task
                    .execution_attempts
                    .iter()
                    .filter(|attempt| attempt.id == request.attempt_id)
                    .count()
                    != 1
            {
                return Err(refused());
            }
            let attempt = task
                .execution_attempts
                .iter_mut()
                .find(|attempt| attempt.id == request.attempt_id)
                .ok_or(PlanError::TaskNotFound)?;
            if attempt.executor_tab_id != request.tab_id
                || attempt.acknowledged_at.is_none()
                || attempt.closed_at.is_some()
                || attempt.retired_at.is_some()
                || attempt.interruption.owner_epoch != request.owner_epoch
                || attempt.interruption.interrupted_at.is_some()
                || attempt.interruption.reason.is_some()
                || attempt.continuation != Default::default()
                || attempt.competition_group_id.is_some()
                || attempt.mesh_peer.is_some()
                || attempt.provider.as_deref() != Some(request.provider.as_str())
                || attempt.agent_type.as_deref() != Some(request.provider.as_str())
                || attempt.role.as_deref() != Some("worker")
                || attempt
                    .parent_tab_id
                    .as_deref()
                    .is_some_and(|id| id != request.creator_tab_id)
                || attempt
                    .orchestrator_tab_id
                    .as_deref()
                    .is_some_and(|id| id != request.creator_tab_id)
                || attempt.plan_skill_manifest.is_empty()
                || attempt.plan_skill_manifest != manifest
            {
                return Err(refused());
            }
            match (
                attempt.session_id.as_deref(),
                attempt.session_binding_nonce.as_deref(),
            ) {
                (None, None) => {
                    attempt.session_id = Some(request.session_id.clone());
                    attempt.session_binding_nonce = Some(request.nonce.clone());
                    task.interruption.owner_session_id = Some(request.session_id.clone());
                    completed = Some(attempt.clone());
                    Ok(Some(()))
                }
                (Some(session), Some(nonce))
                    if session == request.session_id
                        && nonce == request.nonce
                        && task.interruption.owner_session_id.as_deref()
                            == Some(request.session_id.as_str()) =>
                {
                    completed = Some(attempt.clone());
                    Ok(None)
                }
                _ => Err(refused()),
            }
        })?;
        completed.ok_or_else(refused)
    }
}

fn refused() -> PlanError {
    PlanError::Io("retained launch cannot complete this exact acknowledged attempt binding".into())
}

#[cfg(test)]
mod tests;
