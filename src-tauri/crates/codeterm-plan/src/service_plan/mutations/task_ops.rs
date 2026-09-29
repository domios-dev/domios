use super::*;
use serde::Serialize;

/// `Append` is the default, non-destructive update shape; `Replace` is the
/// explicit, opt-in destructive one — codeterm#372.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcceptanceCriteriaUpdate {
    Append(Vec<String>),
    Replace(Vec<String>),
}

/// Before/after array lengths for a destructive `Replace`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct AcceptanceCriteriaCounts {
    pub before: usize,
    pub after: usize,
}

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
            let task = crate::plan::task_key::find_task_mut(&mut plan.tasks, task_id)
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
            let task_index = crate::plan::task_key::find_task_index(&p.tasks, task_id)
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
            if let Some(Some(key)) = group {
                super::ensure_group_declared(p, Some(key));
            }
            let task = &mut p.tasks[task_index];
            if let Some(lane) = lane {
                let tab_explicit = lane.tab_id.is_some();
                apply_lane_update(&mut task.lane, lane);
                task.lane = normalize_lane_metadata(std::mem::take(&mut task.lane));
                if tab_explicit {
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
            let task_index = crate::plan::task_key::find_task_index(&p.tasks, task_id)
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
            let task = crate::plan::task_key::find_task_mut(&mut p.tasks, task_id)
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
        let personality = personality
            .map(|name| {
                name.map(crate::service_plan::worker_identity::validate_executor_name)
                    .transpose()
            })
            .transpose()
            .map_err(PlanError::InvalidExecutorName)?;
        self.with_plan(plan_id, now_ms, |p| {
            let task = crate::plan::task_key::find_task_mut(&mut p.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            if let Some(name) = personality {
                task.personality = name;
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
            let task = crate::plan::task_key::find_task_mut(&mut p.tasks, task_id)
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
            let task = crate::plan::task_key::find_task_mut(&mut p.tasks, task_id)
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
        // Explicit --tab overwrites the executor binding.
        let tab_explicit = lane.tab_id.is_some();
        let write = self.with_plan(plan_id, now_ms, |p| {
            let task = crate::plan::task_key::find_task_mut(&mut p.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            apply_lane_update(&mut task.lane, lane);
            task.lane = normalize_lane_metadata(std::mem::take(&mut task.lane));
            if tab_explicit && is_terminal_task_status(&task.status) {
                let assigned = task.assigned_tab_id.clone();
                super::task_attempts::mark_task_assignment_closed(
                    task,
                    assigned.as_deref(),
                    now_ms,
                );
                clear_task_assignment(task);
            } else if tab_explicit {
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
        acceptance_criteria: Option<AcceptanceCriteriaUpdate>,
        test_plan: Option<Option<&str>>,
        qa_plan: Option<Option<&str>>,
        correctness: Option<Option<&str>>,
        now_ms: u64,
    ) -> Result<Option<AcceptanceCriteriaCounts>, PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            let task = crate::plan::task_key::find_task_mut(&mut p.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            let criteria_counts = acceptance_criteria.map(|update| {
                let before = task.acceptance_criteria.len();
                match update {
                    AcceptanceCriteriaUpdate::Append(new_items) => {
                        task.acceptance_criteria.extend(new_items);
                    }
                    AcceptanceCriteriaUpdate::Replace(new_items) => {
                        task.acceptance_criteria = new_items;
                    }
                }
                AcceptanceCriteriaCounts {
                    before,
                    after: task.acceptance_criteria.len(),
                }
            });
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
            Ok(criteria_counts)
        })
    }

    #[cfg(any(test, feature = "test-support"))]
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

    #[cfg(any(test, feature = "test-support"))]
    pub fn assign_task_with_role(
        &self,
        plan_id: &str,
        task_id: &str,
        tab: &str,
        peer: Option<&str>,
        role: Option<&str>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.assign_task_with_optional_binding(
            plan_id, task_id, tab, peer, role, None, None, now_ms,
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
            let (assigned, dispatch_attempt_id) =
                crate::plan::task_key::find_task(&p.tasks, task_id)
                    .map(|task| {
                        let assigned = task.assigned_tab_id.clone();
                        let attempt_id = task
                            .execution_attempts
                            .iter()
                            .rev()
                            .find(|attempt| {
                                assigned.as_deref() == Some(attempt.executor_tab_id.as_str())
                                    && attempt.closed_at.is_none()
                                    && attempt.retired_at.is_none()
                            })
                            .map(|attempt| attempt.id.clone());
                        (assigned, attempt_id)
                    })
                    .ok_or(PlanError::TaskNotFound)?;
            let blocked_reason = dispatch_attempt_id.as_deref().and_then(|attempt_id| {
                super::super::task_dispatch::restore_blocked_unblock(p, task_id, attempt_id)
            });
            let task = crate::plan::task_key::find_task_mut(&mut p.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            super::task_attempts::mark_task_assignment_closed(task, assigned.as_deref(), now_ms);
            clear_task_assignment(task);
            task.status = if blocked_reason.is_some() {
                crate::plan::TaskStatus::Blocked
            } else {
                crate::plan::TaskStatus::Pending
            };
            if blocked_reason.is_some() {
                task.status_label = blocked_reason;
            }
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
mod fill_lane_tests;
