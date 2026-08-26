use super::*;

impl PlanStore {
    pub fn add_task_external_refs(
        &self,
        plan_id: &str,
        task_id: &str,
        values: &[String],
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let additions = crate::service_plan::external_refs::canonicalize_external_refs(values)?;
        if additions.is_empty() {
            return Ok(());
        }
        let expected = additions.clone();
        let write = self.with_plan(plan_id, now_ms, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            crate::service_plan::external_refs::merge_external_refs(
                &mut task.external_refs,
                additions,
            );
            task.updated_at = now_ms;
            Ok(())
        });
        pending_after_verified(
            write,
            self.verify_task_external_refs_added(plan_id, task_id, &expected),
            None,
        )
    }

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
        self.update_task_with_lane(
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
            section_ids,
            parent_task_id,
            group,
            tags,
            primary,
            handle,
            None,
            now_ms,
        )
    }

    pub fn update_task_with_lane(
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
        lane: Option<super::TaskLaneUpdate>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        // Presence, not vocabulary: any lexical floor is satisfiable by a longer
        // shrug, and what makes a role real is not machine-checkable.
        if executor_role.is_some_and(|role| role.is_none_or(|s| s.trim().is_empty())) {
            return Err(PlanError::ExecutorRoleRequired);
        }
        let expected_sections = section_ids.clone();
        let expected_lane = lane.clone();
        let write = self.with_plan(plan_id, now_ms, |p| {
            let task_index = p
                .tasks
                .iter()
                .position(|t| t.id == task_id || t.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            let task_id = p.tasks[task_index].id.clone();
            let parsed_status = status.map(parse_task_status).transpose()?;
            if let Some(next) = &parsed_status {
                validate_task_status_transition_with_lane(
                    p,
                    &task_id,
                    &p.tasks[task_index].status,
                    next,
                    status_label,
                    lane.as_ref(),
                )?;
            }
            let task = &mut p.tasks[task_index];
            if let Some(lane) = lane {
                let tab_explicit = lane.tab_id.is_some();
                apply_lane_update(&mut task.lane, lane);
                task.lane = normalize_lane_metadata(std::mem::take(&mut task.lane));
                if tab_explicit || task.assigned_tab_id.is_none() {
                    task.assigned_tab_id = task.lane.tab_id.clone();
                }
            }
            if let Some(n) = name {
                task.name = n.into();
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
                let entering_review =
                    task.status != TaskStatus::InReview && status == TaskStatus::InReview;
                let closing_review =
                    task.status == TaskStatus::InReview && is_terminal_task_status(&status);
                if task.status != status {
                    task.status = status;
                    task.status_changed_at = now_ms;
                }
                if task.status == TaskStatus::InProgress {
                    p.mode = PlanMode::Execution;
                }
                super::qa::normalize_task_review_fields(task);
                if task.status != TaskStatus::Paused {
                    task.pause_reason = None;
                }
                if is_terminal_task_status(&task.status) {
                    let assigned = task.assigned_tab_id.clone();
                    super::task_attempts::mark_task_assignment_closed(
                        task,
                        assigned.as_deref(),
                        now_ms,
                    );
                    clear_task_assignment(task);
                }
                if entering_review {
                    crate::service_plan::review_gate::fix_oracle_and_consume_override(task, now_ms);
                    crate::service_plan::review_gate::spend_review_credit(&mut p.review_policy);
                } else if closing_review {
                    crate::service_plan::review_gate::earn_review_credit(&mut p.review_policy);
                }
            }
            let task = &mut p.tasks[task_index];
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
        let verified = self.verify_task_updated(
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
        );
        let verified = match (verified, expected_lane.as_ref()) {
            (Ok(()), Some(lane)) => self.verify_task_lane_updated(plan_id, task_id, lane),
            (result, _) => result,
        };
        pending_after_verified(write, verified, Some(task_id.to_string()))
    }

    pub fn update_task_dependencies(
        &self,
        plan_id: &str,
        task_id: &str,
        depends_on: Vec<String>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let write: Result<Vec<String>, PlanError> = self.with_plan(plan_id, now_ms, |p| {
            let task_index = p
                .tasks
                .iter()
                .position(|t| t.id == task_id || t.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            let canonical_task_id = p.tasks[task_index].id.clone();
            let normalized = validate_task_dependencies(p, &canonical_task_id, &depends_on)?;
            let task = &mut p.tasks[task_index];
            task.depends_on = normalized.clone();
            task.updated_at = now_ms;
            Ok(normalized)
        });
        // depends_on refs (e.g. handles) resolve to canonical task ids during the
        // write; verify against the resolved form or readback always fails.
        let (write, verify_depends_on) = match write {
            Ok(resolved) => (Ok(()), resolved),
            Err(err) => (Err(err), normalized_dependency_ids(&depends_on)),
        };
        pending_after_verified(
            write,
            self.verify_task_dependencies_updated(plan_id, task_id, &verify_depends_on),
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

    pub fn update_task_personality(
        &self,
        plan_id: &str,
        task_id: &str,
        personality: Option<Option<&str>>,
        provenance: Option<Option<crate::plan::IdentityProvenance>>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            let task = p
                .tasks
                .iter_mut()
                .find(|t| t.id == task_id || t.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            if let Some(name) = personality {
                task.personality = name.map(str::to_string);
            }
            if let Some(provenance) = provenance {
                task.personality_provenance = provenance;
            }
            task.updated_at = now_ms;
            Ok(())
        })
    }

    /// Capture report branch and peer values into empty lane fields.
    /// Reports never change executor bindings.
    pub fn record_report_lane_truth(
        &self,
        plan_id: &str,
        task_id: &str,
        branch: Option<&str>,
        peer: Option<&str>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let branch = non_empty_arg(branch);
        let peer = non_empty_arg(peer);
        if branch.is_none() && peer.is_none() {
            return Ok(());
        }
        self.with_plan(plan_id, now_ms, |p| {
            let task = p
                .tasks
                .iter_mut()
                .find(|t| t.id == task_id || t.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            if is_terminal_task_status(&task.status) {
                return Ok(());
            }
            let mut touched = false;
            if let Some(branch) = branch {
                if task.lane.branch.as_deref().unwrap_or("").is_empty() {
                    task.lane.branch = Some(branch.to_string());
                    touched = true;
                }
            }
            if let Some(peer) = peer {
                if task.peer.as_deref().unwrap_or("").is_empty() {
                    task.peer = Some(peer.to_string());
                    touched = true;
                }
            }
            if touched {
                task.lane = normalize_lane_metadata(std::mem::take(&mut task.lane));
                task.updated_at = now_ms;
            }
            Ok(())
        })
    }

    /// Recording discipline: at report / status-to-active time, capture lane
    /// truth from the caller's live context — but only into EMPTY fields, never
    /// overwriting an explicit prior value. This is the fix for tasks going
    /// in_progress with no tab/branch/peer recorded.
    pub fn fill_task_lane_if_empty(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: Option<&str>,
        branch: Option<&str>,
        peer: Option<&str>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let tab_id = non_empty_arg(tab_id);
        let branch = non_empty_arg(branch);
        let peer = non_empty_arg(peer);
        if tab_id.is_none() && branch.is_none() && peer.is_none() {
            return Ok(());
        }
        self.with_plan(plan_id, now_ms, |p| {
            let task = p
                .tasks
                .iter_mut()
                .find(|t| t.id == task_id || t.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            if is_terminal_task_status(&task.status) {
                return Ok(());
            }
            let mut touched = false;
            if let Some(tab) = tab_id {
                if task.lane.tab_id.as_deref().unwrap_or("").is_empty() {
                    task.lane.tab_id = Some(tab.to_string());
                    touched = true;
                }
                if task.assigned_tab_id.as_deref().unwrap_or("").is_empty() {
                    task.assigned_tab_id = Some(tab.to_string());
                    touched = true;
                }
            }
            if let Some(branch) = branch {
                if task.lane.branch.as_deref().unwrap_or("").is_empty() {
                    task.lane.branch = Some(branch.to_string());
                    touched = true;
                }
            }
            if let Some(peer) = peer {
                if task.peer.as_deref().unwrap_or("").is_empty() {
                    task.peer = Some(peer.to_string());
                    touched = true;
                }
            }
            if touched {
                task.lane = normalize_lane_metadata(std::mem::take(&mut task.lane));
                task.updated_at = now_ms;
            }
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
        // Explicit --tab overwrites the executor binding; fill-if-empty is only for implicit capture.
        let tab_explicit = lane.tab_id.is_some();
        let write = self.with_plan(plan_id, now_ms, |p| {
            let task = p
                .tasks
                .iter_mut()
                .find(|t| t.id == task_id || t.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            apply_lane_update(&mut task.lane, lane);
            task.lane = normalize_lane_metadata(std::mem::take(&mut task.lane));
            if is_terminal_task_status(&task.status) {
                let assigned = task.assigned_tab_id.clone();
                super::task_attempts::mark_task_assignment_closed(
                    task,
                    assigned.as_deref(),
                    now_ms,
                );
                clear_task_assignment(task);
            } else if tab_explicit || task.assigned_tab_id.is_none() {
                task.assigned_tab_id = task.lane.tab_id.clone();
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
        qa_plan: Option<Option<&str>>,
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
            if let Some(plan) = qa_plan {
                task.qa_plan = plan.map(str::to_string);
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
        tab: &str,
        peer: Option<&str>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.assign_task_with_role(plan_id, task_id, tab, peer, None, now_ms)
    }

    pub fn assign_task_with_role(
        &self,
        plan_id: &str,
        task_id: &str,
        tab: &str,
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
            task.assigned_tab_id = Some(tab.into());
            task.lane.tab_id = Some(tab.into());
            task.taken_by_role = role.map(str::to_string);
            task.peer = peer.map(|s| s.into());
            task.updated_at = now_ms;
            let attempt =
                super::task_attempts::new_assignment_attempt(task, tab, peer, role, now_ms);
            task.execution_attempts.push(attempt);
            Ok(())
        });
        pending_after_verified(
            write,
            self.verify_task_assigned(plan_id, task_id, tab, peer),
            Some(task_id.to_string()),
        )
    }

    /// Undo an optimistic dispatch (assign + in_progress) when a later pipeline
    /// step fails — clears the tab and returns the task to pending.
    pub fn revert_task_dispatch(
        &self,
        plan_id: &str,
        task_id: &str,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            let task = p
                .tasks
                .iter_mut()
                .find(|t| t.id == task_id || t.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            let assigned = task.assigned_tab_id.clone();
            super::task_attempts::mark_task_assignment_closed(task, assigned.as_deref(), now_ms);
            clear_task_assignment(task);
            task.status = crate::plan::TaskStatus::Pending;
            task.status_changed_at = now_ms;
            task.updated_at = now_ms;
            Ok(())
        })
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

fn non_empty_arg(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|s| !s.is_empty())
}

#[cfg(test)]
mod fill_lane_tests {
    use crate::plan::{OrchestratorRef, Plan, PlanMode, TaskStatus};
    use crate::service_plan::{NewTask, PlanStore, TaskLaneUpdate};
    use std::sync::{Arc, Barrier};

    fn setup() -> (tempfile::TempDir, PlanStore, String, String) {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "L".into(),
            dir.path().display().to_string(),
            OrchestratorRef::default(),
        );
        plan.id = "pl_lane".into();
        plan.mode = PlanMode::Execution;
        store.save(&mut plan, 1).unwrap();
        let task = store
            .add_task(
                &plan.id,
                NewTask {
                    name: "t".into(),
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        (dir, store, plan.id, task)
    }

    #[test]
    fn fills_empty_tab_branch_and_mirrors_assigned() {
        let (_d, store, plan, task) = setup();
        store
            .fill_task_lane_if_empty(&plan, &task, Some("tab-1"), Some("lane/x"), None, 5)
            .unwrap();
        let p = store.get(&plan).unwrap();
        let t = p.tasks.iter().find(|t| t.id == task).unwrap();
        assert_eq!(t.lane.tab_id.as_deref(), Some("tab-1"));
        assert_eq!(t.assigned_tab_id.as_deref(), Some("tab-1"));
        assert_eq!(t.lane.branch.as_deref(), Some("lane/x"));
    }

    #[test]
    fn never_overwrites_existing_values() {
        let (_d, store, plan, task) = setup();
        store
            .fill_task_lane_if_empty(&plan, &task, Some("first"), Some("branch-a"), None, 5)
            .unwrap();
        store
            .fill_task_lane_if_empty(&plan, &task, Some("second"), Some("branch-b"), None, 6)
            .unwrap();
        let p = store.get(&plan).unwrap();
        let t = p.tasks.iter().find(|t| t.id == task).unwrap();
        assert_eq!(t.assigned_tab_id.as_deref(), Some("first"));
        assert_eq!(t.lane.branch.as_deref(), Some("branch-a"));
    }

    #[test]
    fn revert_dispatch_clears_tab_and_returns_pending() {
        let (_d, store, plan, task) = setup();
        store
            .assign_task_with_role(&plan, &task, "worker-tab", None, Some("worker"), 5)
            .unwrap();
        store
            .update_task(
                &plan,
                &task,
                None,
                None,
                None,
                None,
                Some("in_progress"),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                &[],
                None,
                None,
                6,
            )
            .unwrap();
        store.revert_task_dispatch(&plan, &task, 7).unwrap();
        let p = store.get(&plan).unwrap();
        let t = p.tasks.iter().find(|t| t.id == task).unwrap();
        assert!(t.assigned_tab_id.is_none());
        assert!(t.lane.tab_id.is_none());
        assert_eq!(t.status, crate::plan::TaskStatus::Pending);
    }

    #[test]
    fn blank_args_are_noop() {
        let (_d, store, plan, task) = setup();
        store
            .fill_task_lane_if_empty(&plan, &task, Some("  "), Some(""), None, 5)
            .unwrap();
        let p = store.get(&plan).unwrap();
        let t = p.tasks.iter().find(|t| t.id == task).unwrap();
        assert!(t.assigned_tab_id.is_none());
        assert!(t.lane.branch.is_none());
    }

    #[test]
    fn explicit_tab_update_overwrites_assigned_tab() {
        // #209 regression: an explicit lane tab update must rebind the executor,
        // not silently no-op because assigned_tab_id was already set.
        let (_d, store, plan, task) = setup();
        store
            .fill_task_lane_if_empty(&plan, &task, Some("old-tab"), None, None, 5)
            .unwrap();
        let update = crate::service_plan::TaskLaneUpdate {
            tab_id: Some(Some("new-tab".into())),
            ..Default::default()
        };
        store.update_task_lane(&plan, &task, update, 6).unwrap();
        let p = store.get(&plan).unwrap();
        let t = p.tasks.iter().find(|t| t.id == task).unwrap();
        assert_eq!(t.lane.tab_id.as_deref(), Some("new-tab"));
        assert_eq!(t.assigned_tab_id.as_deref(), Some("new-tab"));
    }

    #[test]
    fn implicit_lane_update_still_fills_only_empty() {
        let (_d, store, plan, task) = setup();
        store
            .fill_task_lane_if_empty(&plan, &task, Some("keeper"), None, None, 5)
            .unwrap();
        let update = crate::service_plan::TaskLaneUpdate {
            branch: Some(Some("lane/x".into())),
            ..Default::default()
        };
        store.update_task_lane(&plan, &task, update, 6).unwrap();
        let p = store.get(&plan).unwrap();
        let t = p.tasks.iter().find(|t| t.id == task).unwrap();
        assert_eq!(t.assigned_tab_id.as_deref(), Some("keeper"));
        assert_eq!(t.lane.branch.as_deref(), Some("lane/x"));
    }

    fn set_status(
        store: &PlanStore,
        plan: &str,
        task: &str,
        status: &str,
        label: Option<Option<&str>>,
        ts: u64,
    ) -> Result<(), crate::service_plan::PlanError> {
        store.update_task(
            plan,
            task,
            None,
            None,
            None,
            None,
            Some(status),
            label,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
            None,
            None,
            ts,
        )
    }

    #[test]
    fn in_progress_requires_tab_invariant() {
        let (_d, store, plan, task) = setup();
        let err = set_status(&store, &plan, &task, "in_progress", None, 5).unwrap_err();
        assert!(matches!(
            err,
            crate::service_plan::PlanError::InProgressRequiresTab
        ));
        store
            .fill_task_lane_if_empty(&plan, &task, Some("tab-1"), None, None, 6)
            .unwrap();
        set_status(&store, &plan, &task, "in_progress", None, 7).unwrap();
    }

    #[test]
    fn lane_and_in_progress_commit_as_one_plan_mutation() {
        let (_d, store, plan, task) = setup();
        let error = set_status(&store, &plan, &task, "in_progress", None, 5).unwrap_err();
        assert!(matches!(
            error,
            crate::service_plan::PlanError::InProgressRequiresTab
        ));

        store
            .update_task_with_lane(
                &plan,
                &task,
                None,
                None,
                None,
                None,
                Some("in_progress"),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                &[],
                None,
                None,
                Some(TaskLaneUpdate {
                    tab_id: Some(Some("worker-tab".into())),
                    ..Default::default()
                }),
                6,
            )
            .unwrap();

        let task = store.show_task(&task).unwrap().task;
        assert_eq!(task.status, TaskStatus::InProgress);
        assert_eq!(task.lane.tab_id.as_deref(), Some("worker-tab"));
        assert_eq!(task.assigned_tab_id.as_deref(), Some("worker-tab"));
    }

    #[test]
    fn rejected_status_does_not_leave_a_half_applied_lane() {
        let (_d, store, plan, task) = setup();
        let error = store
            .update_task_with_lane(
                &plan,
                &task,
                None,
                None,
                None,
                None,
                Some("blocked"),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                &[],
                None,
                None,
                Some(TaskLaneUpdate {
                    tab_id: Some(Some("must-not-commit".into())),
                    ..Default::default()
                }),
                6,
            )
            .unwrap_err();
        assert!(matches!(
            error,
            crate::service_plan::PlanError::BlockedRequiresLabel
        ));
        let task = store.show_task(&task).unwrap().task;
        assert!(task.lane.tab_id.is_none());
        assert!(task.assigned_tab_id.is_none());
        assert_eq!(task.status, TaskStatus::Pending);
    }

    #[test]
    fn late_validation_failure_rolls_back_lane_and_status_together() {
        let (_d, store, plan, task) = setup();
        let error = store
            .update_task_with_lane(
                &plan,
                &task,
                None,
                None,
                None,
                None,
                Some("in_progress"),
                None,
                Some("not-a-priority"),
                None,
                None,
                None,
                None,
                None,
                None,
                &[],
                None,
                None,
                Some(TaskLaneUpdate {
                    tab_id: Some(Some("must-roll-back".into())),
                    ..Default::default()
                }),
                6,
            )
            .unwrap_err();
        assert!(matches!(error, crate::service_plan::PlanError::BadPriority));
        let task = store.show_task(&task).unwrap().task;
        assert_eq!(task.status, TaskStatus::Pending);
        assert!(task.lane.tab_id.is_none());
        assert!(task.assigned_tab_id.is_none());
    }

    #[test]
    fn concurrent_same_plan_task_mutations_keep_exact_status_buckets() {
        let (_d, store, plan, first_task) = setup();
        let mut tasks = vec![first_task];
        for index in 1..8 {
            tasks.push(
                store
                    .add_task(
                        &plan,
                        NewTask {
                            name: format!("task-{index}"),
                            acceptance_criteria: vec!["observable result".into()],
                            test_plan: Some("cargo test exact".into()),
                            ..Default::default()
                        },
                        3 + index,
                    )
                    .unwrap(),
            );
        }
        let store = Arc::new(store);
        let start = Arc::new(Barrier::new(tasks.len()));
        let statuses = [
            ("pending", None),
            ("pending", None),
            ("pending", None),
            ("in_review", None),
            ("blocked", Some("waiting on dependency")),
            ("done", None),
            ("done", None),
            ("done", None),
        ];
        let mut workers = Vec::new();
        for (index, (task, (status, label))) in tasks.into_iter().zip(statuses).enumerate() {
            let store = Arc::clone(&store);
            let plan = plan.clone();
            let start = Arc::clone(&start);
            workers.push(std::thread::spawn(move || {
                start.wait();
                store.update_task(
                    &plan,
                    &task,
                    None,
                    None,
                    None,
                    None,
                    Some(status),
                    label.map(Some),
                    None,
                    Some(&format!("concurrent-mutation-{index}")),
                    None,
                    None,
                    None,
                    None,
                    None,
                    &[],
                    None,
                    None,
                    100 + index as u64,
                )
            }));
        }
        for worker in workers {
            worker.join().unwrap().unwrap();
        }

        let plan = store.get(&plan).unwrap();
        let counts = plan.tasks.iter().fold([0usize; 4], |mut counts, task| {
            match task.status {
                TaskStatus::Pending => counts[0] += 1,
                TaskStatus::InReview => counts[1] += 1,
                TaskStatus::Blocked => counts[2] += 1,
                TaskStatus::Done => counts[3] += 1,
                _ => panic!("unexpected status: {:?}", task.status),
            }
            counts
        });
        assert_eq!(counts, [3, 1, 1, 3]);
    }

    #[test]
    fn blocked_requires_label_invariant() {
        let (_d, store, plan, task) = setup();
        let err = set_status(&store, &plan, &task, "blocked", None, 5).unwrap_err();
        assert!(matches!(
            err,
            crate::service_plan::PlanError::BlockedRequiresLabel
        ));
        set_status(
            &store,
            &plan,
            &task,
            "blocked",
            Some(Some("waiting on QA")),
            6,
        )
        .unwrap();
    }

    #[test]
    fn explicit_dependencies_roundtrip_without_parking() {
        let (_d, store, plan, dependency) = setup();
        let child = store
            .add_task(
                &plan,
                crate::service_plan::NewTask {
                    name: "child".into(),
                    depends_on: vec![dependency.clone()],
                    ..Default::default()
                },
                3,
            )
            .unwrap();
        let p = store.get(&plan).unwrap();
        let child = p.tasks.iter().find(|t| t.id == child).unwrap();
        assert_eq!(child.depends_on, vec![dependency]);
        assert_eq!(child.status, crate::plan::TaskStatus::Pending);
    }

    #[test]
    fn update_dependencies_sets_and_clears_roundtrip() {
        let (_d, store, plan, dependency) = setup();
        let child = store
            .add_task(
                &plan,
                crate::service_plan::NewTask {
                    name: "child".into(),
                    ..Default::default()
                },
                3,
            )
            .unwrap();
        store
            .update_task_dependencies(&plan, &child, vec![dependency.clone()], 4)
            .unwrap();
        let p = store.get(&plan).unwrap();
        assert_eq!(
            p.tasks.iter().find(|t| t.id == child).unwrap().depends_on,
            vec![dependency.clone()]
        );
        store
            .update_task_dependencies(&plan, &child, vec![], 5)
            .unwrap();
        let p = store.get(&plan).unwrap();
        assert!(p
            .tasks
            .iter()
            .find(|t| t.id == child)
            .unwrap()
            .depends_on
            .is_empty());
    }

    #[test]
    fn invalid_dependency_ids_self_and_cycles_are_rejected() {
        let (_d, store, plan, parent) = setup();
        let child = store
            .add_task(
                &plan,
                crate::service_plan::NewTask {
                    name: "child".into(),
                    ..Default::default()
                },
                3,
            )
            .unwrap();
        let err = store
            .update_task_dependencies(&plan, &child, vec!["missing".into()], 4)
            .unwrap_err();
        assert!(matches!(
            err,
            crate::service_plan::PlanError::DependencyTaskNotFound { .. }
        ));
        let err = store
            .update_task_dependencies(&plan, &child, vec![child.clone()], 5)
            .unwrap_err();
        assert!(matches!(
            err,
            crate::service_plan::PlanError::DependencySelfReference
        ));
        store
            .update_task_dependencies(&plan, &child, vec![parent.clone()], 6)
            .unwrap();
        let err = store
            .update_task_dependencies(&plan, &parent, vec![child.clone()], 7)
            .unwrap_err();
        assert!(matches!(
            err,
            crate::service_plan::PlanError::DependencyCycle
        ));
    }

    #[test]
    fn depends_on_resolves_task_handle_to_canonical_id() {
        let (_d, store, plan, _parent) = setup();
        let handle_target = store
            .add_task(
                &plan,
                crate::service_plan::NewTask {
                    name: "target".into(),
                    handle: Some("target-handle".into()),
                    ..Default::default()
                },
                3,
            )
            .unwrap();
        let child = store
            .add_task(
                &plan,
                crate::service_plan::NewTask {
                    name: "child".into(),
                    depends_on: vec!["target-handle".into()],
                    ..Default::default()
                },
                4,
            )
            .unwrap();
        let p = store.get(&plan).unwrap();
        let child_task = p.tasks.iter().find(|t| t.id == child).unwrap();
        assert_eq!(child_task.depends_on, vec![handle_target.clone()]);

        let other = store
            .add_task(
                &plan,
                crate::service_plan::NewTask {
                    name: "other".into(),
                    ..Default::default()
                },
                5,
            )
            .unwrap();
        store
            .update_task_dependencies(&plan, &other, vec!["target-handle".into()], 6)
            .unwrap();
        let p = store.get(&plan).unwrap();
        assert_eq!(
            p.tasks.iter().find(|t| t.id == other).unwrap().depends_on,
            vec![handle_target]
        );
    }
}
