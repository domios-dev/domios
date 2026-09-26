use super::interruption::{authorize_task_mutation, TaskMutationAuthority};
use super::readback::pending_after_verified;
use super::task_attempts::{new_assignment_attempt, record_noncompetitive_replacement_attempt};
use super::{PlanError, PlanStore};
use crate::plan::TaskStatus;

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
        let write = self.with_plan(plan_id, now_ms, |plan| {
            let index = crate::plan::task_key::find_task_index(&plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            if let Some(authority) = authority {
                authorize_task_mutation(plan, &plan.tasks[index], authority)?;
            }
            if binding.is_some() && plan.tasks[index].status != TaskStatus::Pending {
                return Err(PlanError::Io("local reassignment requires a pending task; use review continuation or respawn".into()));
            }
            let task = &mut plan.tasks[index];
            task.assigned_tab_id = Some(tab.into());
            task.lane.tab_id = Some(tab.into());
            task.taken_by_role = role.map(str::to_string);
            task.peer = peer.map(str::to_string);
            task.updated_at = now_ms;
            let mut attempt = new_assignment_attempt(task, tab, peer, role, now_ms);
            if let Some(binding) = binding {
                attempt.session_id = Some(binding.session_id.clone());
                attempt.session_binding_nonce = Some(binding.session_binding_nonce.clone());
                if let Some(provider) = binding.provider.as_ref() {
                    attempt.provider = Some(provider.clone());
                    attempt.agent_type = Some(provider.clone());
                }
                if let Some(model) = binding.model.as_ref() {
                    attempt.model = Some(model.clone());
                }
                attempt.session_model = binding.session_model.clone();
                attempt.reasoning_effort = binding.reasoning_effort.clone();
                task.interruption.owner_epoch += 1;
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
        });
        pending_after_verified(
            write,
            self.verify_task_assigned(plan_id, task_id, tab, peer),
            Some(task_id.to_string()),
        )
    }
}

#[cfg(test)]
mod tests;
