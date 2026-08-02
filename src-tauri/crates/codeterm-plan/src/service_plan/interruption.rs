use super::{PlanError, PlanStore};
use crate::plan::{ExecutionIntent, InterruptionCheckpoint, ProviderAllowance, TaskStatus};
use sha2::{Digest, Sha256};

#[cfg(test)]
mod tests;

pub fn checkpoint_digest(diff_artifact: &str) -> String {
    format!("{:x}", Sha256::digest(diff_artifact.as_bytes()))
}

pub fn provider_allowance_preflight(
    intent: &ExecutionIntent,
    allowance: Option<&ProviderAllowance>,
    now_ms: u64,
) -> Result<(), PlanError> {
    if *intent != ExecutionIntent::Mutating {
        return Ok(());
    }
    let allowance = allowance.ok_or_else(|| {
        PlanError::Io("mutating dispatch requires provider allowance telemetry".into())
    })?;
    if allowance.permits(intent, now_ms) {
        Ok(())
    } else {
        Err(PlanError::Io(
            "mutating dispatch denied by stale or below-reserve provider allowance".into(),
        ))
    }
}

impl PlanStore {
    pub fn record_dispatch_policy(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        intent: ExecutionIntent,
        allowance: Option<ProviderAllowance>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        provider_allowance_preflight(&intent, allowance.as_ref(), now_ms)?;
        self.with_plan(plan_id, now_ms, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            let attempt = task
                .execution_attempts
                .iter_mut()
                .find(|attempt| attempt.id == attempt_id && attempt.closed_at.is_none())
                .ok_or(PlanError::TaskNotFound)?;
            task.interruption.execution_intent = intent;
            task.interruption.allowance = allowance;
            task.interruption.owner_epoch = task.interruption.owner_epoch.max(1);
            task.interruption.owner_tab_id = Some(attempt.executor_tab_id.clone());
            task.interruption.owner_session_id = attempt.session_id.clone();
            attempt.interruption.owner_epoch = task.interruption.owner_epoch;
            task.updated_at = now_ms;
            Ok(())
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn begin_task_interruption(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        owner_tab_id: &str,
        owner_session_id: &str,
        owner_epoch: u64,
        reason: &str,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            if task.interruption.owner_epoch != owner_epoch
                || task.interruption.owner_tab_id.as_deref() != Some(owner_tab_id)
                || task.interruption.owner_session_id.as_deref() != Some(owner_session_id)
            {
                return Err(PlanError::Io("stale interruption owner".into()));
            }
            let attempt = task
                .execution_attempts
                .iter_mut()
                .find(|attempt| attempt.id == attempt_id)
                .filter(|attempt| {
                    attempt.closed_at.is_none()
                        && attempt.reported_at.is_none()
                        && attempt.executor_tab_id == owner_tab_id
                        && attempt.session_id.as_deref() == Some(owner_session_id)
                        && attempt.interruption.owner_epoch == owner_epoch
                })
                .ok_or_else(|| PlanError::Io("interruption attempt authority changed".into()))?;
            attempt.closed_at = Some(now_ms);
            attempt.interruption.interrupted_at = Some(now_ms);
            attempt.interruption.reason = Some(reason.to_string());
            task.interruption.handoff_required = true;
            task.interruption.checkpoint = None;
            task.status = TaskStatus::Blocked;
            task.status_label = Some("checkpoint_sealing".into());
            task.assigned_tab_id = None;
            task.lane.tab_id = None;
            task.updated_at = now_ms;
            Ok(())
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn finish_task_interruption(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        owner_epoch: u64,
        checkpoint: InterruptionCheckpoint,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        if checkpoint.diff_sha256 != checkpoint_digest(&checkpoint.diff_artifact)
            || checkpoint.task_id != task_id
            || checkpoint.attempt_id != attempt_id
            || checkpoint.owner_epoch != owner_epoch
        {
            return Err(PlanError::Io("interruption checkpoint verification failed".into()));
        }
        self.with_plan(plan_id, now_ms, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            if !task.interruption.handoff_required
                || task.interruption.owner_epoch != owner_epoch
                || task.status_label.as_deref() != Some("checkpoint_sealing")
            {
                return Err(PlanError::Io("interruption checkpoint ownership changed".into()));
            }
            let attempt = task
                .execution_attempts
                .iter_mut()
                .find(|attempt| {
                    attempt.id == attempt_id
                        && attempt.closed_at.is_some()
                        && attempt.interruption.interrupted_at.is_some()
                        && attempt.interruption.owner_epoch == owner_epoch
                })
                .ok_or_else(|| PlanError::Io("interrupted attempt changed".into()))?;
            attempt.interruption.checkpoint_digest = Some(checkpoint.diff_sha256.clone());
            task.interruption.checkpoint = Some(checkpoint);
            task.status_label = Some("handoff_required".into());
            task.updated_at = now_ms;
            Ok(())
        })
    }

    pub fn mark_interruption_checkpoint_failed(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        owner_epoch: u64,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            let exact = task.execution_attempts.iter().any(|attempt| {
                attempt.id == attempt_id
                    && attempt.interruption.owner_epoch == owner_epoch
                    && attempt.interruption.interrupted_at.is_some()
            });
            if !exact || !task.interruption.handoff_required {
                return Err(PlanError::Io("interruption failure authority changed".into()));
            }
            task.status_label = Some("handoff_checkpoint_failed".into());
            task.updated_at = now_ms;
            Ok(())
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn interrupt_task_attempt(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        owner_tab_id: &str,
        owner_session_id: &str,
        owner_epoch: u64,
        reason: &str,
        checkpoint: InterruptionCheckpoint,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.begin_task_interruption(
            plan_id,
            task_id,
            attempt_id,
            owner_tab_id,
            owner_session_id,
            owner_epoch,
            reason,
            now_ms,
        )?;
        self.finish_task_interruption(
            plan_id,
            task_id,
            attempt_id,
            owner_epoch,
            checkpoint,
            now_ms,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn adopt_interrupted_task(
        &self,
        plan_id: &str,
        task_id: &str,
        expected_digest: &str,
        expected_base_head: &str,
        new_tab_id: &str,
        new_session_id: &str,
        now_ms: u64,
    ) -> Result<(u64, String), PlanError> {
        let mut adopted = None;
        self.with_plan(plan_id, now_ms, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            let checkpoint = task
                .interruption
                .checkpoint
                .as_ref()
                .filter(|checkpoint| task.interruption.handoff_required)
                .ok_or_else(|| PlanError::Io("task has no adoptable checkpoint".into()))?;
            if checkpoint.diff_sha256 != expected_digest
                || checkpoint.diff_sha256 != checkpoint_digest(&checkpoint.diff_artifact)
                || checkpoint.base_head != expected_base_head
            {
                return Err(PlanError::Io("checkpoint digest or base changed".into()));
            }
            let epoch = task.interruption.owner_epoch.saturating_add(1);
            let mut attempt = super::task_attempts::new_assignment_attempt(
                task,
                new_tab_id,
                None,
                Some("worker"),
                now_ms,
            );
            attempt.session_id = Some(new_session_id.to_string());
            attempt.interruption.owner_epoch = epoch;
            let attempt_id = attempt.id.clone();
            task.execution_attempts.push(attempt);
            task.interruption.owner_epoch = epoch;
            task.interruption.owner_tab_id = Some(new_tab_id.to_string());
            task.interruption.owner_session_id = Some(new_session_id.to_string());
            task.interruption.handoff_required = false;
            task.assigned_tab_id = Some(new_tab_id.to_string());
            task.lane.tab_id = Some(new_tab_id.to_string());
            task.status = TaskStatus::Pending;
            task.status_label = Some("awaiting_worker_ack".into());
            task.updated_at = now_ms;
            adopted = Some((epoch, attempt_id));
            Ok(())
        })?;
        adopted.ok_or_else(|| PlanError::Io("adoption was not persisted".into()))
    }

    pub fn verify_task_owner_epoch(
        &self,
        task_id: &str,
        tab_id: &str,
        session_id: &str,
        owner_epoch: u64,
    ) -> Result<(), PlanError> {
        let detail = self.show_task(task_id).ok_or(PlanError::TaskNotFound)?;
        let owner = &detail.task.interruption;
        if owner.handoff_required
            || owner.owner_epoch != owner_epoch
            || owner.owner_tab_id.as_deref() != Some(tab_id)
            || owner.owner_session_id.as_deref() != Some(session_id)
        {
            return Err(PlanError::Io("stale task owner epoch".into()));
        }
        Ok(())
    }
}
