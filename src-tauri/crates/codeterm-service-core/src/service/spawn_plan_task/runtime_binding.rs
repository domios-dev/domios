use super::*;
use crate::service::agent_runtime::plan_warning::PlanBindingFailure;

impl ServiceContext {
    pub(crate) fn prepare_runtime_worker_binding(
        &self,
        params: &SpawnAgentParams,
        tab_id: &str,
    ) -> Result<(), String> {
        let mut prepared = params.clone();
        if prepared.remote_task_binding.is_none()
            && prepared.plan_task.is_none()
            && prepared.task_launch_authority.is_some()
        {
            prepared.plan_task = prepared.task_id.clone();
        }
        self.bind_worker_plan_task_or_rollback_tab(&prepared, tab_id)
    }

    pub(crate) fn finalize_runtime_worker_session(
        &self,
        tab_id: &str,
        thread: &str,
        nonce: &str,
    ) -> Result<(), String> {
        self.report_runtime_worker_session_finalization(tab_id, thread, nonce);
        Ok(())
    }

    pub(crate) fn report_runtime_worker_session_finalization(
        &self,
        tab_id: &str,
        thread: &str,
        nonce: &str,
    ) -> Option<String> {
        let reason = self
            .try_finalize_runtime_worker_session(tab_id, thread, nonce)
            .err()?;
        self.warn_runtime_plan_binding(
            tab_id,
            crate::service::agent_runtime::plan_warning::PlanBindingWarningStage::Finalization,
            &reason,
        )
    }

    fn try_finalize_runtime_worker_session(
        &self,
        tab_id: &str,
        thread: &str,
        nonce: &str,
    ) -> Result<(), PlanBindingFailure> {
        self.restore_runtime_attempt_binding(tab_id)?;
        self.bind_planner_session(tab_id);
        if self.carry_authorized_runtime_restart_binding(tab_id, thread, nonce)? {
            return Ok(());
        }
        let (binding, restored, remote) = self
            .mux
            .read()
            .get_tab(tab_id)
            .map(|tab| {
                (
                    tab.task_binding.clone(),
                    tab.restored_from_snapshot,
                    tab.report_target_mesh_peer.is_some(),
                )
            })
            .unwrap_or_default();
        let Some(binding) = binding else {
            return Ok(());
        };
        if binding.plan_id.is_none() && binding.task_id.is_none() && binding.attempt_id.is_none() {
            return Ok(());
        }
        let (Some(plan), Some(task), Some(attempt)) =
            (binding.plan_id, binding.task_id, binding.attempt_id)
        else {
            return Err("Runtime task binding incomplete".into());
        };
        let Some(detail) = self.plan_store.show_task(&task) else {
            return if remote {
                Ok(())
            } else {
                Err(PlanBindingFailure::bookkeeping(
                    "Runtime task disappeared before readiness",
                ))
            };
        };
        if detail.plan_id != plan {
            return Err("Runtime task plan changed".into());
        }
        let existing = detail
            .task
            .execution_attempts
            .iter()
            .find(|entry| entry.id == attempt && entry.executor_tab_id == tab_id)
            .ok_or("Runtime task attempt changed")?;
        if needs_attempt_finalization(
            existing.session_id.as_deref(),
            existing.session_binding_nonce.as_deref(),
            existing.closed_at.is_some(),
            existing.retired_at.is_some(),
            restored,
            thread,
            nonce,
        )? {
            if restored {
                self.plan_store.bind_restored_runtime_task_session(
                    crate::service::plan::task_attempts::RestoredRuntimeSessionBinding {
                        plan_id: plan,
                        task_id: task,
                        attempt_id: attempt,
                        executor_tab_id: tab_id.into(),
                        thread: thread.into(),
                        nonce: nonce.into(),
                        expected_thread: existing.session_id.clone(),
                        expected_nonce: existing.session_binding_nonce.clone(),
                        owner_epoch: detail.task.interruption.owner_epoch,
                    },
                    epoch_ms(),
                )
            } else {
                self.plan_store.finish_task_dispatch(
                    &plan,
                    &task,
                    tab_id,
                    &attempt,
                    thread,
                    nonce,
                    detail.task.lane,
                    epoch_ms(),
                )
            }
            .map_err(PlanBindingFailure::from_plan_error)?;
        }
        Ok(())
    }
}

fn needs_attempt_finalization(
    saved_thread: Option<&str>,
    saved_nonce: Option<&str>,
    closed: bool,
    retired: bool,
    restored: bool,
    thread: &str,
    nonce: &str,
) -> Result<bool, String> {
    if retired {
        return Err("Runtime task attempt retired".into());
    }
    if saved_thread.is_some_and(|bound| bound != thread)
        || (!restored && saved_nonce.is_some_and(|bound| bound != nonce))
    {
        return Err("Runtime task session rebound".into());
    }
    if closed {
        return if saved_thread == Some(thread) {
            Ok(false)
        } else {
            Err("Closed runtime attempt has no bound conversation".into())
        };
    }
    Ok(saved_thread.is_none() || saved_nonce != Some(nonce))
}

#[cfg(test)]
mod tests;
