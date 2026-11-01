use super::*;

impl PlanStore {
    pub fn record_task_respawn_attempt(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        draft: &TaskRespawnDraft,
        role: Option<&str>,
        now_ms: u64,
    ) -> Result<TaskAttemptCreated, PlanError> {
        let mut created = None;
        let writer_turn = self.mutation_writer.enter();
        if draft.source_attempt.placement == crate::plan::WorkerPlacement::Shared {
            let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
            let task = crate::plan::task_key::find_task(&plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            super::super::shared_checkout::validate_shared_files(
                &self.checkout_peer_plans(&plan),
                &plan,
                task,
                &task.files,
            )
            .map_err(PlanError::InvalidPlanText)?;
        }
        self.with_plan_in_writer(&writer_turn, plan_id, now_ms, |plan| {
            let task_index = crate::plan::task_key::find_task_index(&plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            super::super::task_dispatch::task_respawn_status(&plan.tasks[task_index].status)?;
            ensure_respawn_source_current(&plan.tasks[task_index], &draft.source_attempt.id)?;
            if matches!(
                plan.tasks[task_index].status,
                TaskStatus::Pending | TaskStatus::Blocked
            ) {
                super::super::review_gate::validate_dispatch_review_capacity(
                    plan,
                    &plan.tasks[task_index],
                )?;
            }
            let attempt_id = draft.attempt_id.clone();
            super::super::task_dispatch::record_blocked_task_unblock(
                plan,
                task_index,
                "respawn",
                Some(&attempt_id),
                now_ms,
            );
            let task = &mut plan.tasks[task_index];
            let source = task
                .execution_attempts
                .iter_mut()
                .find(|attempt| attempt.id == draft.source_attempt.id)
                .ok_or(PlanError::TaskNotFound)?;
            source.retired_at = Some(now_ms);
            if source.closed_at.is_none() {
                source.closed_at = Some(now_ms);
            }
            task.assigned_tab_id = Some(tab_id.into());
            task.lane.tab_id = Some(tab_id.into());
            task.taken_by_role = role.map(str::to_string);
            task.peer = None;
            task.status = TaskStatus::Pending;
            task.status_changed_at = now_ms;
            task.status_label = Some("starting".into());
            task.updated_at = now_ms;
            let mut attempt =
                new_assignment_attempt_with_id(task, tab_id, None, role, Some(&attempt_id), now_ms);
            attempt.provider = Some(draft.provider.clone());
            attempt.agent_type = Some(draft.provider.clone());
            attempt.model = draft.model.clone();
            attempt.provider_resolved_model = draft.model.clone();
            attempt.reasoning_effort = draft.reasoning_effort.clone();
            attempt.executor_title = draft.title.clone();
            attempt.parent_tab_id = draft.parent_tab_id.clone();
            attempt.plan_skill_manifest = draft.plan_skill_manifest.clone();
            attempt.authorized_gates = draft.authorized_gates.clone();
            attempt.placement = draft.source_attempt.placement;
            attempt.checkout_path = draft.source_attempt.checkout_path.clone();
            attempt.shared_start_sha = draft.source_attempt.shared_start_sha.clone();
            record_noncompetitive_replacement_attempt(
                &mut task.execution_attempts,
                attempt.clone(),
                now_ms,
            );
            created = Some(attempt);
            Ok(())
        })?;
        Ok(TaskAttemptCreated {
            plan_id: plan_id.into(),
            task_id: task_id.into(),
            attempt: created.expect("attempt set before successful write"),
        })
    }
}
