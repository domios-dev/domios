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
        self.record_task_respawn_attempt_with_undo(plan_id, task_id, tab_id, draft, role, now_ms)
            .map(|(created, _)| created)
    }

    pub fn record_task_respawn_attempt_with_undo(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        draft: &TaskRespawnDraft,
        role: Option<&str>,
        now_ms: u64,
    ) -> Result<(TaskAttemptCreated, super::super::DispatchUndo), PlanError> {
        let mut created = None;
        let mut undo = None;
        let writer_turn = self.mutation_writer.enter();
        if draft.source_attempt.placement == crate::plan::WorkerPlacement::Shared {
            let task = self.task_checkout_projection(plan_id, task_id, false)?;
            super::super::shared_checkout::validate_shared_files_at(
                &self.checkout_peer_facts(&task.checkout)?,
                plan_id,
                &task.id,
                &super::super::shared_checkout::canonical_path(&task.checkout),
                &task.files,
            )
            .map_err(PlanError::InvalidPlanText)?;
        }
        self.with_task_rows_in_writer(
            &writer_turn,
            plan_id,
            task_id,
            super::super::store_db::hot_write::TaskHistory::Respawn(
                draft.source_attempt.id.clone(),
            ),
            now_ms,
            |edit| {
                super::super::task_dispatch::task_respawn_status(&edit.task.status)?;
                ensure_respawn_source_current(&edit.task, &draft.source_attempt.id)?;
                edit.warn_dispatch =
                    matches!(edit.task.status, TaskStatus::Pending | TaskStatus::Blocked);
                let attempt_id = draft.attempt_id.clone();
                undo = Some(super::super::DispatchUndo::capture(
                    plan_id,
                    &edit.mode,
                    &edit.task,
                    &attempt_id,
                    Some(&draft.source_attempt.id),
                ));
                edit.unblock("respawn", &attempt_id, now_ms);
                let task = &mut edit.task;
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
                let mut attempt = new_assignment_attempt_with_id(
                    task,
                    tab_id,
                    None,
                    role,
                    Some(&attempt_id),
                    now_ms,
                );
                attempt.provider = Some(draft.provider.clone());
                attempt.agent_type = Some(draft.provider.clone());
                attempt.model = draft.model.clone();
                attempt.launch_profile = draft.launch_profile.clone();
                attempt.provider_resolved_model = draft.model.clone();
                attempt.reasoning_effort = draft.reasoning_effort.clone();
                attempt.executor_title = draft.title.clone();
                attempt.parent_tab_id = draft.parent_tab_id.clone();
                attempt.plan_skill_manifest = draft.plan_skill_manifest.clone();
                attempt.authorized_gates = draft.authorized_gates.clone();
                attempt.placement = draft.source_attempt.placement;
                attempt.execution_intent = draft.source_attempt.execution_intent.clone();
                attempt.checkout_path = draft.source_attempt.checkout_path.clone();
                attempt.shared_start_sha = draft.source_attempt.shared_start_sha.clone();
                attempt.shared_start_files = draft.source_attempt.shared_start_files.clone();
                record_noncompetitive_replacement_attempt(
                    &mut task.execution_attempts,
                    attempt.clone(),
                    now_ms,
                );
                undo.as_mut()
                    .expect("undo captured before respawn mutation")
                    .finish(&edit.mode, task);
                created = Some(attempt);
                Ok(())
            },
        )?;
        Ok((
            TaskAttemptCreated {
                plan_id: plan_id.into(),
                task_id: task_id.into(),
                attempt: created.expect("attempt set before successful write"),
            },
            undo.expect("undo captured before successful respawn"),
        ))
    }
}
