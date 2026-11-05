use super::super::{PlanError, PlanStore};
use crate::plan::{TaskExecutionAttempt, TaskStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
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

pub struct CompletedDispatchBinding {
    pub attempt: TaskExecutionAttempt,
    request: CompleteAcknowledgedDispatchBinding,
    prior_owner_session: Option<Option<String>>,
}

impl PlanStore {
    pub fn complete_acknowledged_dispatch_binding(
        &self,
        request: CompleteAcknowledgedDispatchBinding,
        now_ms: u64,
    ) -> Result<TaskExecutionAttempt, PlanError> {
        self.complete_acknowledged_dispatch_binding_with_undo(request, now_ms)
            .map(|receipt| receipt.attempt)
    }

    pub fn complete_acknowledged_dispatch_binding_with_undo(
        &self,
        request: CompleteAcknowledgedDispatchBinding,
        now_ms: u64,
    ) -> Result<CompletedDispatchBinding, PlanError> {
        let plan_id = request.plan_id.clone();
        let mut completed = None;
        let mut prior_owner_session = None;
        let turn = self.mutation_writer.enter();
        let manifest = super::super::plan_skills::plan_skill_manifest(self, &plan_id)?;
        self.with_task_rows_in_writer(
            &turn,
            &plan_id,
            &request.task_id,
            super::super::store_db::hot_write::TaskHistory::Binding {
                attempt: request.attempt_id.clone(),
                tab: request.tab_id.clone(),
            },
            now_ms,
            |edit| {
                if edit.plan_status != "active"
                    || edit.team_id.as_deref() != Some(request.team_id.as_str())
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
                for owner in &edit.tab_owners {
                    let attempt_id = owner.attempt_id.clone().ok_or_else(refused)?;
                    owners.push((owner.plan_id.clone(), owner.task_id.clone(), attempt_id));
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
                let task = &mut edit.task;
                if task.id != request.task_id {
                    return Err(PlanError::TaskNotFound);
                }
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
                        .filter(|attempt| {
                            attempt.closed_at.is_none() && attempt.retired_at.is_none()
                        })
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
                        prior_owner_session = Some(task.interruption.owner_session_id.clone());
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
            },
        )?;
        Ok(CompletedDispatchBinding {
            attempt: completed.ok_or_else(refused)?,
            request,
            prior_owner_session,
        })
    }

    pub fn undo_completed_dispatch_binding(
        &self,
        receipt: CompletedDispatchBinding,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let Some(prior_owner_session) = receipt.prior_owner_session else {
            return Ok(());
        };
        let request = receipt.request;
        self.with_task_rows(
            &request.plan_id,
            &request.task_id,
            super::super::store_db::hot_write::TaskHistory::Attempt(request.attempt_id.clone()),
            now_ms,
            |edit| {
                let task = &mut edit.task;
                if task.id != request.task_id {
                    return Err(PlanError::TaskNotFound);
                }
                let attempt = task
                    .execution_attempts
                    .iter_mut()
                    .find(|attempt| attempt.id == request.attempt_id)
                    .ok_or(PlanError::TaskNotFound)?;
                if task.interruption.owner_epoch != request.owner_epoch
                    || task.interruption.owner_tab_id.as_deref() != Some(request.tab_id.as_str())
                    || task.interruption.owner_session_id.as_deref()
                        != Some(request.session_id.as_str())
                    || attempt.executor_tab_id != request.tab_id
                    || attempt.interruption.owner_epoch != request.owner_epoch
                    || attempt.session_id.as_deref() != Some(request.session_id.as_str())
                    || attempt.session_binding_nonce.as_deref() != Some(request.nonce.as_str())
                {
                    return Err(PlanError::Io(
                        "dispatch binding rollback conflict: exact attempt authority changed"
                            .into(),
                    ));
                }
                attempt.session_id = None;
                attempt.session_binding_nonce = None;
                task.interruption.owner_session_id = prior_owner_session;
                Ok(Some(()))
            },
        )
        .map(|_| ())
    }
}

fn refused() -> PlanError {
    PlanError::Io("retained launch cannot complete this exact acknowledged attempt binding".into())
}

#[cfg(test)]
mod tests;
