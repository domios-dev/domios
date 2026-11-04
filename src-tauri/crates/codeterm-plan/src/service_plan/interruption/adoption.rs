use super::*;
use crate::plan::{Task, TaskExecutionAttempt};

#[derive(Clone, PartialEq)]
struct AdoptionFields {
    epoch: u64,
    owner_tab: Option<String>,
    owner_session: Option<String>,
    handoff_required: bool,
    assigned_tab: Option<String>,
    lane_tab: Option<String>,
    status: TaskStatus,
    status_label: Option<String>,
}
impl AdoptionFields {
    fn read(task: &Task) -> Self {
        Self {
            epoch: task.interruption.owner_epoch,
            owner_tab: task.interruption.owner_tab_id.clone(),
            owner_session: task.interruption.owner_session_id.clone(),
            handoff_required: task.interruption.handoff_required,
            assigned_tab: task.assigned_tab_id.clone(),
            lane_tab: task.lane.tab_id.clone(),
            status: task.status.clone(),
            status_label: task.status_label.clone(),
        }
    }
    fn apply(self, task: &mut Task) {
        task.interruption.owner_epoch = self.epoch;
        task.interruption.owner_tab_id = self.owner_tab;
        task.interruption.owner_session_id = self.owner_session;
        task.interruption.handoff_required = self.handoff_required;
        task.assigned_tab_id = self.assigned_tab;
        task.lane.tab_id = self.lane_tab;
        task.status = self.status;
        task.status_label = self.status_label;
    }
}

pub struct CheckpointAdoption {
    plan_id: String,
    task_id: String,
    before: AdoptionFields,
    after: AdoptionFields,
    attempt: TaskExecutionAttempt,
}
impl CheckpointAdoption {
    pub fn outcome(&self) -> (u64, String) {
        (self.after.epoch, self.attempt.id.clone())
    }
}

impl PlanStore {
    #[allow(clippy::too_many_arguments)]
    pub fn adopt_interrupted_task(
        &self,
        plan_id: &str,
        task_id: &str,
        expected_digest: &str,
        expected_base_head: &str,
        new_tab_id: &str,
        binding: &super::super::task_assignment::AssignmentRuntimeBinding,
        now_ms: u64,
    ) -> Result<(u64, String), PlanError> {
        self.adopt_interrupted_task_with_undo(
            plan_id,
            task_id,
            expected_digest,
            expected_base_head,
            new_tab_id,
            binding,
            now_ms,
        )
        .map(|receipt| receipt.outcome())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn adopt_interrupted_task_with_undo(
        &self,
        plan_id: &str,
        task_id: &str,
        expected_digest: &str,
        expected_base_head: &str,
        new_tab_id: &str,
        binding: &super::super::task_assignment::AssignmentRuntimeBinding,
        now_ms: u64,
    ) -> Result<CheckpointAdoption, PlanError> {
        let mut adopted = None;
        self.with_task_rows(
            plan_id,
            task_id,
            super::super::store_db::hot_write::TaskHistory::Core,
            now_ms,
            |edit| {
                let task = &mut edit.task;
                let checkpoint = task
                    .interruption
                    .checkpoint
                    .as_ref()
                    .filter(|_| task.interruption.handoff_required)
                    .ok_or_else(|| PlanError::Io("task has no adoptable checkpoint".into()))?;
                if checkpoint.diff_sha256 != expected_digest
                    || checkpoint.diff_sha256 != checkpoint_digest(&checkpoint.diff_artifact)
                    || checkpoint.base_head != expected_base_head
                {
                    return Err(PlanError::Io("checkpoint digest or base changed".into()));
                }
                let before = AdoptionFields::read(task);
                let epoch = super::next_owner_epoch(task.interruption.owner_epoch)?;
                let mut attempt = super::super::task_attempts::new_assignment_attempt(
                    task,
                    new_tab_id,
                    None,
                    Some("worker"),
                    now_ms,
                );
                attempt.session_id = Some(binding.session_id.clone());
                attempt.session_binding_nonce = Some(binding.session_binding_nonce.clone());
                attempt.provider = binding.provider.clone();
                attempt.agent_type = binding.provider.clone();
                attempt.requested_model = binding.model.clone();
                attempt.model = binding.model.clone();
                attempt.provider_resolved_model = binding.model.clone();
                attempt.session_model = binding.session_model.clone();
                attempt.reasoning_effort = binding.reasoning_effort.clone();
                attempt.interruption.owner_epoch = epoch;
                task.execution_attempts.push(attempt.clone());
                task.interruption.owner_epoch = epoch;
                task.interruption.owner_tab_id = Some(new_tab_id.to_string());
                task.interruption.owner_session_id = Some(binding.session_id.clone());
                task.interruption.handoff_required = false;
                task.assigned_tab_id = Some(new_tab_id.to_string());
                task.lane.tab_id = Some(new_tab_id.to_string());
                task.status = TaskStatus::Pending;
                task.status_label = Some("awaiting_worker_ack".into());
                task.updated_at = now_ms;
                adopted = Some(CheckpointAdoption {
                    plan_id: plan_id.into(),
                    task_id: task.id.clone(),
                    before,
                    after: AdoptionFields::read(task),
                    attempt,
                });
                Ok(())
            },
        )?;
        adopted.ok_or_else(|| PlanError::Io("adoption was not persisted".into()))
    }

    pub fn undo_checkpoint_adoption(
        &self,
        receipt: CheckpointAdoption,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let plan_id = receipt.plan_id.clone();
        let task_id = receipt.task_id.clone();
        self.with_task_rows(
            &plan_id,
            &task_id,
            super::super::store_db::hot_write::TaskHistory::Respawn(receipt.attempt.id.clone()),
            now_ms,
            |edit| {
                let task = &mut edit.task;
                if task.id != receipt.task_id {
                    return Err(PlanError::TaskNotFound);
                }
                if AdoptionFields::read(task) != receipt.after
                    || task.execution_attempts.last() != Some(&receipt.attempt)
                {
                    return Err(PlanError::Io(
                        "checkpoint adoption rollback conflict: new owner or attempt changed"
                            .into(),
                    ));
                }
                edit.removed_attempts.push(receipt.attempt.id.clone());
                receipt.before.apply(task);
                task.updated_at = now_ms;
                Ok(())
            },
        )
    }
}
