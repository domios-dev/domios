use super::lane_metadata::normalize_lane_metadata;
use super::readback::pending_after_verified;
use super::{PlanError, PlanStore};

impl PlanStore {
    #[allow(clippy::too_many_arguments)]
    pub fn commit_task_dispatch(
        &self,
        plan_id: &str,
        task_id: &str,
        pane_id: &str,
        executor_title: &str,
        session_id: &str,
        mut lane: crate::plan::TaskLaneMetadata,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let write = self.with_plan(plan_id, now_ms, |plan| {
            let task_index = plan
                .tasks
                .iter()
                .position(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            lane.pane_id = Some(pane_id.to_string());
            let lane_update = super::TaskLaneUpdate {
                pane_id: Some(Some(pane_id.to_string())),
                ..Default::default()
            };
            super::mutations::validate_task_status_transition_with_lane(
                plan,
                &plan.tasks[task_index].id,
                &plan.tasks[task_index].status,
                &crate::plan::TaskStatus::InProgress,
                None,
                Some(&lane_update),
            )?;
            let task = &mut plan.tasks[task_index];
            task.lane = normalize_lane_metadata(lane);
            task.assigned_pane_id = Some(pane_id.to_string());
            task.taken_by_role = Some("worker".into());
            task.peer = None;
            let mut attempt = super::task_attempts::new_assignment_attempt(
                task,
                pane_id,
                None,
                Some("worker"),
                now_ms,
            );
            attempt.executor_title = Some(executor_title.to_string());
            attempt.session_id = Some(session_id.to_string());
            task.execution_attempts.push(attempt);
            task.status = crate::plan::TaskStatus::InProgress;
            task.status_changed_at = now_ms;
            task.updated_at = now_ms;
            plan.mode = crate::plan::PlanMode::Execution;
            Ok(())
        });
        let verified = (|| {
            let task = self.read_task_for_verification(plan_id, task_id)?;
            let attempt = task
                .execution_attempts
                .last()
                .ok_or_else(|| Self::task_readback_error("task_dispatch", "attempt missing"))?;
            if task.status != crate::plan::TaskStatus::InProgress
                || task.assigned_pane_id.as_deref() != Some(pane_id)
                || task.lane.pane_id.as_deref() != Some(pane_id)
                || attempt.executor_pane_id != pane_id
                || attempt.executor_title.as_deref() != Some(executor_title)
                || attempt.session_id.as_deref() != Some(session_id)
                || attempt.worktree_path != task.lane.worktree_path
                || attempt.branch != task.lane.branch
                || attempt.base != task.lane.base
                || attempt.destination != task.lane.destination
            {
                return Err(Self::task_readback_error(
                    "task_dispatch",
                    format!("dispatch state did not match for {}", task.id),
                ));
            }
            Ok(())
        })();
        pending_after_verified(write, verified, Some(task_id.to_string()))
    }

    pub fn restore_task_dispatch_snapshot(
        &self,
        plan_id: &str,
        snapshot: &crate::plan::Task,
        plan_mode: crate::plan::PlanMode,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let snapshot = snapshot.clone();
        let task_id = snapshot.id.clone();
        let expected = snapshot.clone();
        let expected_mode = plan_mode.clone();
        let write = self.with_plan(plan_id, now_ms, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id || task.handle.as_deref() == Some(&task_id))
                .ok_or(PlanError::TaskNotFound)?;
            *task = snapshot;
            plan.mode = plan_mode;
            Ok(())
        });
        let verified = self
            .committed_plan_for(plan_id)
            .ok_or(PlanError::NotFound)
            .and_then(|plan| {
                let mode_matches = plan.mode == expected_mode;
                let task = plan
                    .tasks
                    .into_iter()
                    .find(|task| task.id == task_id || task.handle.as_deref() == Some(&task_id))
                    .ok_or(PlanError::TaskNotFound)?;
                if task == expected && mode_matches {
                    Ok(())
                } else {
                    Err(Self::task_readback_error(
                        "task_dispatch_restore",
                        format!("task snapshot did not match for {task_id}"),
                    ))
                }
            });
        pending_after_verified(write, verified, Some(task_id))
    }
}
