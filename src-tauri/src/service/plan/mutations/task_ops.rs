use super::*;

impl PlanStore {
    pub fn update_task(
        &self,
        plan_id: &str,
        task_id: &str,
        name: Option<&str>,
        kind: Option<&str>,
        model: Option<Option<&str>>,
        model_rationale: Option<Option<&str>>,
        status: Option<&str>,
        status_label: Option<Option<&str>>,
        priority: Option<&str>,
        body: Option<&str>,
        intent: Option<Option<&str>>,
        executor_role: Option<Option<&str>>,
        section_ids: Option<Vec<String>>,
        parent_task_id: Option<Option<&str>>,
        group: Option<Option<&str>>,
        tags: &[String],
        primary: Option<&str>,
        handle: Option<&str>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let expected_sections = section_ids.clone();
        let write = self.with_plan(plan_id, now_ms, |p| {
            let task_index = p
                .tasks
                .iter()
                .position(|t| t.id == task_id || t.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            let task_id = p.tasks[task_index].id.clone();
            let parsed_status = status.map(parse_task_status).transpose()?;
            if let Some(next) = &parsed_status {
                validate_task_status_transition(p, &task_id, &p.tasks[task_index].status, next)?;
            }
            let task = &mut p.tasks[task_index];
            if let Some(n) = name {
                task.name = n.into();
                task.handle = None;
            }
            if let Some(k) = kind {
                task.kind = k.into();
            }
            if let Some(m) = model {
                task.model = m.map(|s| s.into());
            }
            if let Some(w) = model_rationale {
                task.model_rationale = w.map(|s| s.into());
            }
            if let Some(status) = parsed_status {
                if task.status != status {
                    task.status = status;
                    task.status_changed_at = now_ms;
                }
                super::qa::normalize_task_review_fields(task);
                if task.status != TaskStatus::Parked {
                    task.park_reason = None;
                }
                if is_terminal_task_status(&task.status) {
                    let assigned = task.assigned_pane_id.clone();
                    super::task_attempts::mark_task_assignment_closed(
                        task,
                        assigned.as_deref(),
                        now_ms,
                    );
                    clear_task_assignment(task);
                }
            }
            if let Some(priority) = priority {
                task.priority = parse_task_priority(priority)?;
            }
            if let Some(l) = status_label {
                task.status_label = l.map(|s| s.into());
            }
            if let Some(b) = body {
                task.body = b.into();
            }
            if let Some(i) = intent {
                task.intent = i.map(|s| s.into());
            }
            if let Some(r) = executor_role {
                task.executor_role = r.map(|s| s.into());
            }
            if let Some(ids) = section_ids {
                task.section_ids = ids;
            }
            if let Some(parent) = parent_task_id {
                task.parent_task_id = parent.map(|s| s.into());
            }
            if let Some(g) = group {
                task.group = g.map(|s| s.into());
            }
            super::tag_model::apply_tags(&mut task.tags, tags, primary);
            if !tags.is_empty() || primary.is_some() {
                task.handle = None;
            }
            if let Some(handle) = handle {
                task.handle = Some(handle.into());
            }
            task.updated_at = now_ms;
            Ok(())
        });
        pending_after_verified(
            write,
            self.verify_task_updated(
                plan_id,
                task_id,
                name,
                kind,
                model,
                model_rationale,
                status,
                status_label,
                priority,
                body,
                intent,
                executor_role,
                expected_sections.as_deref(),
                parent_task_id,
                group,
                handle,
            ),
            Some(task_id.to_string()),
        )
    }

    pub fn update_task_role_guidance(
        &self,
        plan_id: &str,
        task_id: &str,
        role_guidance: Option<Option<&str>>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            let task = p
                .tasks
                .iter_mut()
                .find(|t| t.id == task_id || t.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            if let Some(guidance) = role_guidance {
                task.role_guidance = guidance.map(str::to_string);
            }
            task.updated_at = now_ms;
            Ok(())
        })
    }

    pub fn update_task_lane(
        &self,
        plan_id: &str,
        task_id: &str,
        lane: super::TaskLaneUpdate,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let expected = lane.clone();
        let write = self.with_plan(plan_id, now_ms, |p| {
            let task = p
                .tasks
                .iter_mut()
                .find(|t| t.id == task_id || t.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            apply_lane_update(&mut task.lane, lane);
            task.lane = normalize_lane_metadata(std::mem::take(&mut task.lane));
            if is_terminal_task_status(&task.status) {
                let assigned = task.assigned_pane_id.clone();
                super::task_attempts::mark_task_assignment_closed(
                    task,
                    assigned.as_deref(),
                    now_ms,
                );
                clear_task_assignment(task);
            } else if task.assigned_pane_id.is_none() {
                task.assigned_pane_id = task.lane.pane_id.clone();
            }
            task.updated_at = now_ms;
            Ok(())
        });
        pending_after_verified(
            write,
            self.verify_task_lane_updated(plan_id, task_id, &expected),
            Some(task_id.to_string()),
        )
    }

    pub fn update_task_quality(
        &self,
        plan_id: &str,
        task_id: &str,
        acceptance_criteria: Option<Vec<String>>,
        test_plan: Option<Option<&str>>,
        correctness: Option<Option<&str>>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            let task = p
                .tasks
                .iter_mut()
                .find(|t| t.id == task_id || t.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            if let Some(criteria) = acceptance_criteria {
                task.acceptance_criteria = criteria;
            }
            if let Some(plan) = test_plan {
                task.test_plan = plan.map(str::to_string);
            }
            if let Some(vector) = correctness {
                task.correctness = vector.map(str::to_string);
            }
            task.updated_at = now_ms;
            Ok(())
        })
    }

    pub fn assign_task(
        &self,
        plan_id: &str,
        task_id: &str,
        pane: &str,
        peer: Option<&str>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.assign_task_with_role(plan_id, task_id, pane, peer, None, now_ms)
    }

    pub fn assign_task_with_role(
        &self,
        plan_id: &str,
        task_id: &str,
        pane: &str,
        peer: Option<&str>,
        role: Option<&str>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let write = self.with_plan(plan_id, now_ms, |p| {
            let task = p
                .tasks
                .iter_mut()
                .find(|t| t.id == task_id || t.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            task.assigned_pane_id = Some(pane.into());
            task.lane.pane_id = Some(pane.into());
            task.taken_by_role = role.map(str::to_string);
            task.peer = peer.map(|s| s.into());
            task.updated_at = now_ms;
            let attempt =
                super::task_attempts::new_assignment_attempt(task, pane, peer, role, now_ms);
            task.execution_attempts.push(attempt);
            Ok(())
        });
        pending_after_verified(
            write,
            self.verify_task_assigned(plan_id, task_id, pane, peer),
            Some(task_id.to_string()),
        )
    }

    pub fn update_milestone(
        &self,
        plan_id: &str,
        task_id: &str,
        ms_id: &str,
        kind: Option<&str>,
        text: Option<&str>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            let task = p
                .tasks
                .iter_mut()
                .find(|t| t.id == task_id)
                .ok_or(PlanError::TaskNotFound)?;
            let ms = task
                .milestones
                .iter_mut()
                .find(|m| m.id == ms_id)
                .ok_or(PlanError::NotFound)?;
            if let Some(k) = kind {
                let declared = p.milestone_kinds.iter().any(|mk| mk.key == k);
                if !declared {
                    return Err(PlanError::UndeclaredKind {
                        declared: p.milestone_kinds.iter().map(|mk| mk.key.clone()).collect(),
                    });
                }
                ms.kind = k.into();
            }
            if let Some(t) = text {
                ms.text = t.into();
            }
            Ok(())
        })
    }

    pub fn rm_milestone(
        &self,
        plan_id: &str,
        task_id: &str,
        ms_id: &str,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            let task = p
                .tasks
                .iter_mut()
                .find(|t| t.id == task_id)
                .ok_or(PlanError::TaskNotFound)?;
            let pos = task
                .milestones
                .iter()
                .position(|m| m.id == ms_id)
                .ok_or(PlanError::NotFound)?;
            task.milestones.remove(pos);
            Ok(())
        })
    }
}
