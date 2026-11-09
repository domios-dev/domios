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
        for value in values {
            crate::service_plan::text_bounds::validate_plan_text("external reference", value)?;
        }
        let additions = crate::service_plan::external_refs::canonicalize_external_refs(values)?;
        crate::service_plan::text_bounds::validate_external_refs(&additions)?;
        if additions.is_empty() {
            return Ok(());
        }
        self.with_task_rows(
            plan_id,
            task_id,
            crate::service_plan::store_db::hot_write::TaskHistory::Core,
            now_ms,
            |edit| {
                let task = &mut edit.task;
                crate::service_plan::external_refs::merge_external_refs(
                    &mut task.external_refs,
                    additions,
                );
                task.updated_at = now_ms;
                Ok(())
            },
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
        if let Some(name) = name {
            crate::service_plan::text_bounds::validate_plan_label("task name", name)?;
        }
        for (field, value) in [
            ("task type", kind),
            ("model", model.flatten()),
            ("routing rationale", model_rationale.flatten()),
            ("status label", status_label.flatten()),
            ("task body", body),
            ("intent", intent.flatten()),
            ("executor role", executor_role.flatten()),
            ("parent task id", parent_task_id.flatten()),
            ("task group", group.flatten()),
            ("primary tag", primary),
            ("task handle", handle),
        ] {
            if let Some(value) = value {
                crate::service_plan::text_bounds::validate_plan_text(field, value)?;
            }
        }
        for (field, values) in [
            ("task tag", tags),
            ("section id", section_ids.as_deref().unwrap_or_default()),
        ] {
            for value in values {
                crate::service_plan::text_bounds::validate_plan_text(field, value)?;
            }
        }
        if let Some(lane) = lane.as_ref() {
            crate::service_plan::text_bounds::validate_lane_update(lane)?;
        }
        // Presence, not vocabulary: any lexical floor is satisfiable by a longer
        // shrug, and what makes a role real is not machine-checkable.
        if executor_role.is_some_and(|role| role.is_none_or(|s| s.trim().is_empty())) {
            return Err(PlanError::ExecutorRoleRequired);
        }
        use crate::service_plan::store_db::hot_write::{HotChange, TaskFields};
        self.commit_hot_changes(
            plan_id,
            vec![HotChange::Fields {
                selector: task_id.into(),
                fields: TaskFields {
                    name: name.map(str::to_string),
                    kind: kind.map(str::to_string),
                    model: model.map(|value| value.map(str::to_string)),
                    model_rationale: model_rationale.map(|value| value.map(str::to_string)),
                    status: status.map(str::to_string),
                    status_label: status_label.map(|value| value.map(str::to_string)),
                    priority: priority.map(str::to_string),
                    body: body.map(str::to_string),
                    intent: intent.map(|value| value.map(str::to_string)),
                    executor_role: executor_role.map(|value| value.map(str::to_string)),
                    section_ids,
                    parent_task_id: parent_task_id.map(|value| value.map(str::to_string)),
                    group: group.map(|value| value.map(str::to_string)),
                    tags: tags.to_vec(),
                    remove_tags: Vec::new(),
                    primary: primary.map(str::to_string),
                    handle: handle.map(str::to_string),
                    lane,
                },
            }],
            now_ms,
        )
    }

    pub fn remove_task_tags(
        &self,
        plan_id: &str,
        task_id: &str,
        tags: &[String],
        now_ms: u64,
    ) -> Result<(), PlanError> {
        if tags.is_empty() {
            return Ok(());
        }
        for tag in tags {
            crate::service_plan::text_bounds::validate_plan_text("task tag", tag)?;
        }
        use crate::service_plan::store_db::hot_write::{HotChange, TaskFields};
        self.commit_hot_changes(
            plan_id,
            vec![HotChange::Fields {
                selector: task_id.into(),
                fields: TaskFields {
                    remove_tags: tags.to_vec(),
                    ..Default::default()
                },
            }],
            now_ms,
        )
    }

    pub fn update_task_dependencies(
        &self,
        plan_id: &str,
        task_id: &str,
        depends_on: Vec<String>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let turn = self.mutation_writer.enter();
        let receipt = self
            .database
            .update_dependencies(plan_id, task_id, &depends_on, now_ms)?;
        self.publish_hot_receipt(&turn, receipt);
        Ok(())
    }

    pub fn update_task_role_guidance(
        &self,
        plan_id: &str,
        task_id: &str,
        role_guidance: Option<Option<&str>>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        if let Some(Some(guidance)) = role_guidance {
            crate::service_plan::text_bounds::validate_plan_text("role guidance", guidance)?;
        }
        self.commit_hot_changes(
            plan_id,
            vec![crate::service_plan::store_db::hot_write::HotChange::Task {
                selector: task_id.into(),
                change: crate::service_plan::store_db::hot_write::TaskMetadata::RoleGuidance(
                    role_guidance.map(|value| value.map(str::to_string)),
                ),
            }],
            now_ms,
        )
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
        if let Some(Some(name)) = &personality {
            crate::service_plan::text_bounds::validate_plan_label("personality", name)?;
        }
        self.commit_hot_changes(
            plan_id,
            vec![crate::service_plan::store_db::hot_write::HotChange::Task {
                selector: task_id.into(),
                change: crate::service_plan::store_db::hot_write::TaskMetadata::Personality {
                    name: personality,
                    provenance,
                },
            }],
            now_ms,
        )
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
        self.with_task_rows(
            plan_id,
            task_id,
            crate::service_plan::store_db::hot_write::TaskHistory::Core,
            now_ms,
            |edit| {
                let task = &mut edit.task;
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
            },
        )
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
        self.with_task_rows(
            plan_id,
            task_id,
            crate::service_plan::store_db::hot_write::TaskHistory::Core,
            now_ms,
            |edit| {
                let task = &mut edit.task;
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
            },
        )
    }

    pub fn update_task_lane(
        &self,
        plan_id: &str,
        task_id: &str,
        lane: super::TaskLaneUpdate,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        crate::service_plan::text_bounds::validate_lane_update(&lane)?;
        // Explicit --tab overwrites the executor binding.
        let tab_explicit = lane.tab_id.is_some();
        self.with_task_rows(
            plan_id,
            task_id,
            crate::service_plan::store_db::hot_write::TaskHistory::LiveAndReported,
            now_ms,
            |edit| {
                let task = &mut edit.task;
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
                crate::service_plan::text_bounds::validate_stored_task_text(task)?;
                Ok(())
            },
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
        if let Some(update) = &acceptance_criteria {
            let values = match update {
                AcceptanceCriteriaUpdate::Append(values)
                | AcceptanceCriteriaUpdate::Replace(values) => values,
            };
            for value in values {
                crate::service_plan::text_bounds::validate_plan_text(
                    "acceptance criterion",
                    value,
                )?;
            }
        }
        for (field, value) in [
            ("test plan", test_plan.as_ref().and_then(Option::as_deref)),
            ("QA plan", qa_plan.as_ref().and_then(Option::as_deref)),
            (
                "correctness",
                correctness.as_ref().and_then(Option::as_deref),
            ),
        ] {
            if let Some(value) = value {
                crate::service_plan::text_bounds::validate_plan_text(field, value)?;
            }
        }
        self.with_task_rows(
            plan_id,
            task_id,
            crate::service_plan::store_db::hot_write::TaskHistory::Core,
            now_ms,
            |edit| {
                let task = &mut edit.task;
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
                crate::service_plan::text_bounds::validate_stored_task_text(task)?;
                Ok(criteria_counts)
            },
        )
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
        self.with_task_rows(
            plan_id,
            task_id,
            crate::service_plan::store_db::hot_write::TaskHistory::LiveAndReported,
            now_ms,
            |edit| {
                let task = &mut edit.task;
                let assigned = task.assigned_tab_id.clone();
                edit.rollback_unblock = task
                    .execution_attempts
                    .iter()
                    .rev()
                    .find(|attempt| {
                        assigned.as_deref() == Some(attempt.executor_tab_id.as_str())
                            && attempt.closed_at.is_none()
                            && attempt.retired_at.is_none()
                    })
                    .map(|attempt| attempt.id.clone());
                edit.legacy_dispatch_rollback = true;
                super::task_attempts::mark_task_assignment_closed(
                    task,
                    assigned.as_deref(),
                    now_ms,
                );
                clear_task_assignment(task);
                task.status = crate::plan::TaskStatus::Pending;
                task.status_changed_at = now_ms;
                task.updated_at = now_ms;
                Ok(())
            },
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
        if let Some(kind) = kind {
            crate::service_plan::text_bounds::validate_plan_label("milestone kind", kind)?;
        }
        if let Some(text) = text {
            crate::service_plan::text_bounds::validate_plan_text("milestone text", text)?;
        }
        self.commit_hot_changes(
            plan_id,
            vec![
                crate::service_plan::store_db::hot_write::HotChange::MilestoneEdit {
                    task_id: task_id.into(),
                    milestone_id: ms_id.into(),
                    kind: kind.map(str::to_string),
                    text: text.map(str::to_string),
                    remove: false,
                },
            ],
            now_ms,
        )
    }

    pub fn rm_milestone(
        &self,
        plan_id: &str,
        task_id: &str,
        ms_id: &str,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.commit_hot_changes(
            plan_id,
            vec![
                crate::service_plan::store_db::hot_write::HotChange::MilestoneEdit {
                    task_id: task_id.into(),
                    milestone_id: ms_id.into(),
                    kind: None,
                    text: None,
                    remove: true,
                },
            ],
            now_ms,
        )
    }
}

fn non_empty_arg(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|s| !s.is_empty())
}

#[cfg(test)]
mod fill_lane_tests;
#[cfg(test)]
mod tag_tests;
