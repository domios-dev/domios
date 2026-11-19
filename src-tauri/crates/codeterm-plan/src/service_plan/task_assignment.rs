use super::interruption::{authorize_task_mutation_for_owner, TaskMutationAuthority};
use super::task_attempts::{new_assignment_attempt, record_noncompetitive_replacement_attempt};
use super::{PlanError, PlanStore};
use crate::plan::TaskStatus;
use crate::plan::{ExecutionIntent, TaskExecutionIntent, WorkerPlacement};

pub struct AssignmentRuntimeBinding {
    pub session_id: String,
    pub session_binding_nonce: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub session_model: Option<String>,
    pub reasoning_effort: Option<String>,
}

impl PlanStore {
    #[allow(clippy::too_many_arguments)]
    pub fn assign_task_with_runtime_binding(
        &self,
        plan_id: &str,
        task_id: &str,
        tab: &str,
        peer: Option<&str>,
        role: Option<&str>,
        binding: Option<&AssignmentRuntimeBinding>,
        authority: Option<&TaskMutationAuthority>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        if peer.is_none() && binding.is_none() {
            return Err(PlanError::Io(
                "local assignment requires verified runtime binding".into(),
            ));
        }
        self.assign_task_with_optional_binding(
            plan_id, task_id, tab, peer, role, binding, authority, now_ms,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn assign_task_with_optional_binding(
        &self,
        plan_id: &str,
        task_id: &str,
        tab: &str,
        peer: Option<&str>,
        role: Option<&str>,
        binding: Option<&AssignmentRuntimeBinding>,
        authority: Option<&TaskMutationAuthority>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        if binding.is_some_and(|binding| {
            peer.is_some()
                || binding.session_id.trim().is_empty()
                || binding.session_binding_nonce.trim().is_empty()
        }) {
            return Err(PlanError::Io("invalid assignment runtime binding".into()));
        }
        let requires_repository = self
            .task_dispatch_preflight_projection(plan_id, task_id)?
            .execution_intent
            .requires_repository();
        let shared_inspection = if peer.is_none() && requires_repository {
            Some(self.inspect_shared_dispatch(plan_id, task_id, now_ms)?)
        } else {
            None
        };
        self.with_task_rows(plan_id, task_id, super::store_db::hot_write::TaskHistory::LiveAndReported, now_ms, |edit| {
            if let Some(authority) = authority {
                authorize_task_mutation_for_owner(edit.owner_tab.as_deref(), &edit.task, authority)?;
            }
            if binding.is_some() && edit.task.status != TaskStatus::Pending {
                return Err(PlanError::Io("local reassignment requires a pending task; use review continuation or respawn".into()));
            }
            let task = &mut edit.task;
            task.assigned_tab_id = Some(tab.into());
            task.lane.tab_id = Some(tab.into());
            task.taken_by_role = role.map(str::to_string);
            task.peer = peer.map(str::to_string);
            task.updated_at = now_ms;
            let mut attempt = new_assignment_attempt(task, tab, peer, role, now_ms);
            // Manual assignment records the same readiness identity as
            // dispatch, including shared checkout evidence where applicable.
            match task.execution_intent {
                TaskExecutionIntent::NonRepository => {
                    attempt.placement = WorkerPlacement::NonRepository;
                }
                TaskExecutionIntent::RepositoryMutation => {
                    attempt.execution_intent = Some(ExecutionIntent::Mutating);
                    if peer.is_none() {
                        attempt.placement = WorkerPlacement::Shared;
                        let inspected = shared_inspection.as_ref().ok_or_else(|| {
                            PlanError::InvalidPlanText("shared assignment checkout was not inspected".into())
                        })?;
                        inspected.recheck(&inspected.checkout, &task.files)?;
                        attempt.checkout_path = Some(inspected.checkout.clone());
                        match &inspected.start {
                            super::shared_checkout::SharedStart::Commit(head) => {
                                attempt.shared_start_sha = Some(head.clone());
                            }
                            super::shared_checkout::SharedStart::Files(hashes) => {
                                attempt.shared_start_files = Some(hashes.clone());
                            }
                        }
                    }
                }
            }
            if let Some(binding) = binding {
                attempt.session_id = Some(binding.session_id.clone());
                attempt.session_binding_nonce = Some(binding.session_binding_nonce.clone());
                if let Some(provider) = binding.provider.as_ref() {
                    attempt.provider = Some(provider.clone());
                    attempt.agent_type = Some(provider.clone());
                }
                if let Some(model) = binding.model.as_ref() {
                    attempt.model = Some(model.clone());
                    attempt.provider_resolved_model = Some(model.clone());
                }
                attempt.session_model = binding.session_model.clone();
                attempt.reasoning_effort = binding.reasoning_effort.clone();
                task.interruption.owner_epoch = super::interruption::next_owner_epoch(task.interruption.owner_epoch)?;
                task.interruption.owner_tab_id = Some(tab.into());
                task.interruption.owner_session_id = Some(binding.session_id.clone());
                task.interruption.handoff_required = false;
                attempt.interruption.owner_epoch = task.interruption.owner_epoch;
                task.status_label = Some("awaiting_worker_ack".into());
            }
            record_noncompetitive_replacement_attempt(
                &mut task.execution_attempts,
                attempt,
                now_ms,
            );
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests;
