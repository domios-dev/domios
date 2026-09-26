use super::lane_metadata::{apply_lane_update, normalize_lane_metadata};
use super::readback::pending_after_verified;
use super::{
    qa, section_links, tag_model, task_attempts, NewTask, OpenSubtask, PlanError, PlanStore,
    TaskLaneUpdate,
};
use crate::plan::{
    AgentRef, Goal, Milestone, MilestoneKind, OrchestratorRef, Plan, PlanMode, PlanStatus,
    PlannerRef, PlanningAuthor, PlanningState, PlanningTransition, ReviewEvidence, ReviewVerdict,
    Section, TagFacet, Task, TaskGroup, TaskPriority, TaskStatus, TransferRecord,
};

mod plan_writer;

pub(super) fn gen_id(prefix: &str) -> String {
    crate::plan::ids::short_id(prefix)
}

pub(super) fn plan_contains_id(plan: &Plan, id: &str) -> bool {
    plan.tasks.iter().any(|t| t.id == id)
        || plan
            .tasks
            .iter()
            .any(|t| t.milestones.iter().any(|m| m.id == id))
        || plan.strides.iter().any(|m| m.id == id)
        || section_ids_contains(&plan.sections, id)
}

fn section_matches(section: &Section, selector: &str) -> bool {
    section.id == selector || section.handle.as_deref() == Some(selector)
}

fn section_ids_contains(sections: &[Section], id: &str) -> bool {
    sections
        .iter()
        .any(|s| section_matches(s, id) || section_ids_contains(&s.subsections, id))
}

pub(super) fn push_subsection(
    sections: &mut Vec<Section>,
    parent_id: &str,
    child: Section,
) -> Option<Section> {
    for s in sections.iter_mut() {
        if section_matches(s, parent_id) {
            s.subsections.push(child);
            return None;
        }
    }
    let mut child = child;
    for s in sections.iter_mut() {
        match push_subsection(&mut s.subsections, parent_id, child) {
            None => return None,
            Some(c) => child = c,
        }
    }
    Some(child)
}

pub(super) fn update_section_fields(
    sections: &mut [Section],
    id: &str,
    title: Option<&str>,
    body: Option<&str>,
) -> bool {
    for s in sections.iter_mut() {
        if s.id == id || s.handle.as_deref() == Some(id) {
            if let Some(t) = title {
                s.title = t.into();
                s.handle = None;
            }
            if let Some(b) = body {
                s.body = b.into();
            }
            return true;
        }
        if update_section_fields(&mut s.subsections, id, title, body) {
            return true;
        }
    }
    false
}

pub(super) fn update_section_tags(
    sections: &mut [Section],
    id: &str,
    tags: &[String],
    primary: Option<&str>,
    handle: Option<&str>,
) -> bool {
    for s in sections.iter_mut() {
        if s.id == id || s.handle.as_deref() == Some(id) {
            super::tag_model::apply_tags(&mut s.tags, tags, primary);
            if !tags.is_empty() || primary.is_some() {
                s.handle = None;
            }
            if let Some(handle) = handle {
                s.handle = Some(handle.into());
            }
            return true;
        }
        if update_section_tags(&mut s.subsections, id, tags, primary, handle) {
            return true;
        }
    }
    false
}

pub(super) fn rm_section_tree(sections: &mut Vec<Section>, id: &str) -> bool {
    if let Some(pos) = sections.iter().position(|s| section_matches(s, id)) {
        sections.remove(pos);
        return true;
    }
    sections
        .iter_mut()
        .any(|s| rm_section_tree(&mut s.subsections, id))
}

fn collect_section_tree_ids(sections: &[Section], id: &str, out: &mut Vec<String>) -> bool {
    for section in sections {
        if section.id == id || section.handle.as_deref() == Some(id) {
            collect_all_section_ids(section, out);
            return true;
        }
        if collect_section_tree_ids(&section.subsections, id, out) {
            return true;
        }
    }
    false
}

fn collect_all_section_ids(section: &Section, out: &mut Vec<String>) {
    out.push(section.id.clone());
    for subsection in &section.subsections {
        collect_all_section_ids(subsection, out);
    }
}

pub(super) fn has_section(sections: &[Section], id: &str) -> bool {
    sections.iter().any(|section| {
        section.id == id
            || section.handle.as_deref() == Some(id)
            || has_section(&section.subsections, id)
    })
}

fn parse_task_status(raw: &str) -> Result<TaskStatus, PlanError> {
    serde_json::from_value(serde_json::Value::String(raw.into())).map_err(|_| PlanError::BadStatus)
}

fn parse_plan_mode(raw: &str) -> Result<PlanMode, PlanError> {
    serde_json::from_value(serde_json::Value::String(raw.into())).map_err(|_| PlanError::BadMode)
}

fn parse_task_priority(raw: &str) -> Result<TaskPriority, PlanError> {
    serde_json::from_value(serde_json::Value::String(raw.into()))
        .map_err(|_| PlanError::BadPriority)
}

fn is_terminal_task_status(status: &TaskStatus) -> bool {
    matches!(status, TaskStatus::Done | TaskStatus::Cancelled)
}

pub(super) fn clear_task_assignment(task: &mut Task) {
    task.assigned_tab_id = None;
    task.lane.tab_id = None;
}

fn open_subtasks(plan: &Plan, parent_task_id: &str) -> Vec<OpenSubtask> {
    plan.tasks
        .iter()
        .filter(|task| task.parent_task_id.as_deref() == Some(parent_task_id))
        .filter(|task| !is_terminal_task_status(&task.status))
        .map(|task| OpenSubtask {
            id: task.id.clone(),
            name: task.name.clone(),
            status: task.status.clone(),
            handle: task.handle.clone(),
        })
        .collect()
}

pub(super) fn normalized_dependency_ids(ids: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for id in ids {
        let trimmed = id.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            normalized.push(trimmed.to_string());
        }
    }
    normalized
}

fn dependency_cycle_reaches(
    plan: &Plan,
    current: &str,
    target: &str,
    override_task_id: &str,
    override_dependencies: &[String],
    seen: &mut std::collections::HashSet<String>,
) -> bool {
    if current == target {
        return true;
    }
    if !seen.insert(current.to_string()) {
        return false;
    }
    let Some(task) = plan.tasks.iter().find(|task| task.id == current) else {
        return false;
    };
    let next = if current == override_task_id {
        override_dependencies
    } else {
        &task.depends_on
    };
    next.iter().any(|dep| {
        dependency_cycle_reaches(
            plan,
            dep,
            target,
            override_task_id,
            override_dependencies,
            seen,
        )
    })
}

/// A dependency ref may name a task by full id, handle, or key.
fn resolve_dependency_ref(plan: &Plan, reference: &str) -> Result<String, PlanError> {
    crate::plan::task_key::find_task(&plan.tasks, reference)
        .map(|task| task.id.clone())
        .ok_or_else(|| PlanError::DependencyTaskNotFound {
            task_id: reference.to_string(),
        })
}

pub(super) fn validate_task_dependencies(
    plan: &Plan,
    task_id: &str,
    depends_on: &[String],
) -> Result<Vec<String>, PlanError> {
    let normalized = normalized_dependency_ids(depends_on);
    let mut resolved = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();
    for dependency_ref in &normalized {
        let dependency_id = resolve_dependency_ref(plan, dependency_ref)?;
        if dependency_id == task_id {
            return Err(PlanError::DependencySelfReference);
        }
        if seen_ids.insert(dependency_id.clone()) {
            resolved.push(dependency_id);
        }
    }
    for dependency_id in &resolved {
        let mut seen = std::collections::HashSet::new();
        if dependency_cycle_reaches(plan, dependency_id, task_id, task_id, &resolved, &mut seen) {
            return Err(PlanError::DependencyCycle);
        }
    }
    Ok(resolved)
}

pub(super) fn reject_done_with_open_subtasks(plan: &Plan, task_id: &str) -> Result<(), PlanError> {
    let subtasks = open_subtasks(plan, task_id);
    if subtasks.is_empty() {
        Ok(())
    } else {
        Err(PlanError::OpenSubtasks { subtasks })
    }
}

/// Shared close-transition core (in_review/any -> done) used by both
/// `close_task` and review ACCEPT so the two paths can never drift. Rejects
/// while subtasks are still open; clears assignment and normalizes review state.
pub(crate) fn finalize_task_done(
    plan: &mut Plan,
    task_index: usize,
    now_ms: u64,
) -> Result<(), PlanError> {
    let task_id = plan.tasks[task_index].id.clone();
    reject_done_with_open_subtasks(plan, &task_id)?;
    let task = &mut plan.tasks[task_index];
    if task.status != TaskStatus::Done {
        task.status_changed_at = now_ms;
    }
    task.status = TaskStatus::Done;
    crate::service_plan::qa::normalize_task_review_fields(task);
    crate::service_plan::task_attempts::close_all_live_attempts(
        &mut task.execution_attempts,
        now_ms,
    );
    clear_task_assignment(task);
    task.updated_at = now_ms;
    Ok(())
}

/// The current competition group is the one the newest open competitive attempt
/// belongs to; attempts from a superseded group are foreign.
fn current_competition_group(task: &Task) -> Option<&str> {
    task.execution_attempts
        .iter()
        .rev()
        .find(|attempt| attempt.closed_at.is_none() && attempt.competition_group_id.is_some())
        .and_then(|attempt| attempt.competition_group_id.as_deref())
}

/// A competitive task's durable executor: one exact open current-group attempt
/// that already bound a session or acknowledged.
pub(super) fn live_competitive_executor(task: &Task, attempt_id: &str) -> bool {
    let current_group = current_competition_group(task);
    task.execution_attempts.iter().any(|attempt| {
        attempt.id == attempt_id
            && attempt.closed_at.is_none()
            && !attempt.executor_tab_id.trim().is_empty()
            && attempt.competition_group_id.is_some()
            && attempt.competition_group_id.as_deref() == current_group
            && (attempt.acknowledged_at.is_some()
                || attempt
                    .session_id
                    .as_deref()
                    .is_some_and(|session| !session.trim().is_empty()))
    })
}

/// Who is executing the task at the moment it enters `in_progress`.
pub(super) enum InProgressExecutor<'a> {
    /// A lane tab assignment written in the same transaction.
    LaneAssignment(&'a super::TaskLaneUpdate),
    /// One exact competitive attempt already persisted on the task.
    CompetitiveAttempt(&'a str),
}

pub(super) fn validate_task_status_transition(
    plan: &Plan,
    task_id: &str,
    current: &TaskStatus,
    next: &TaskStatus,
    incoming_label: Option<Option<&str>>,
) -> Result<(), PlanError> {
    validate_task_status_transition_for(plan, task_id, current, next, incoming_label, None)
}

pub(super) fn validate_task_status_transition_with_lane(
    plan: &Plan,
    task_id: &str,
    current: &TaskStatus,
    next: &TaskStatus,
    incoming_label: Option<Option<&str>>,
    lane: Option<&super::TaskLaneUpdate>,
) -> Result<(), PlanError> {
    validate_task_status_transition_for(
        plan,
        task_id,
        current,
        next,
        incoming_label,
        lane.map(InProgressExecutor::LaneAssignment),
    )
}

/// Entry for competitive startup, where progression is authorized by the exact
/// live attempt itself rather than by a task-level tab assignment.
pub(super) fn validate_competitive_attempt_status_transition(
    plan: &Plan,
    task_id: &str,
    current: &TaskStatus,
    next: &TaskStatus,
    attempt_id: &str,
) -> Result<(), PlanError> {
    validate_task_status_transition_for(
        plan,
        task_id,
        current,
        next,
        None,
        Some(InProgressExecutor::CompetitiveAttempt(attempt_id)),
    )
}

fn validate_task_status_transition_for(
    plan: &Plan,
    task_id: &str,
    current: &TaskStatus,
    next: &TaskStatus,
    incoming_label: Option<Option<&str>>,
    executor: Option<InProgressExecutor<'_>>,
) -> Result<(), PlanError> {
    let status_unchanged = current == next;
    let invariants_on = std::env::var("CODETERM_STATE_INVARIANTS").map_or(true, |v| v != "off");
    if invariants_on {
        let task = plan.tasks.iter().find(|t| t.id == task_id);
        if matches!(next, TaskStatus::InProgress) {
            let task_level_tab = task
                .and_then(|t| t.assigned_tab_id.as_deref().or(t.lane.tab_id.as_deref()))
                .is_some_and(|p| !p.trim().is_empty());
            let has_tab = match executor {
                Some(InProgressExecutor::LaneAssignment(update)) => match update.tab_id.as_ref() {
                    Some(tab) => tab.as_deref().is_some_and(|p| !p.trim().is_empty()),
                    None => task_level_tab,
                },
                Some(InProgressExecutor::CompetitiveAttempt(attempt_id)) => {
                    task.is_some_and(|task| live_competitive_executor(task, attempt_id))
                }
                None => task_level_tab,
            };
            if !has_tab {
                return Err(PlanError::InProgressRequiresTab);
            }
            // Only fresh dispatch (claiming new capacity) is throttled; a task returning to in_progress after a review bounce already holds its slot and must not get stuck re-queued behind its own review.
            if matches!(current, TaskStatus::Pending) {
                if let Some(task) = task {
                    super::review_gate::validate_dispatch_review_capacity(plan, task)?;
                }
            }
        }
        if matches!(next, TaskStatus::InReview) {
            if let Some(task) = task {
                super::review_gate::validate_enter_review(task)?;
            }
        }
        if matches!(next, TaskStatus::Blocked) {
            let incoming = incoming_label
                .flatten()
                .map(str::trim)
                .filter(|l| !l.is_empty());
            let existing = task
                .and_then(|t| t.status_label.as_deref())
                .map(str::trim)
                .filter(|l| !l.is_empty());
            if incoming.is_none() && existing.is_none() {
                return Err(PlanError::BlockedRequiresLabel);
            }
        }
    }
    if status_unchanged {
        return Ok(());
    }
    if matches!(current, TaskStatus::InReview) && matches!(next, TaskStatus::Paused) {
        return Err(PlanError::ReviewHoldRequired);
    }
    if matches!(current, TaskStatus::InReview) {
        return Err(PlanError::TransitionDenied {
            from: current.clone(),
            to: next.clone(),
        });
    }
    if matches!(next, TaskStatus::Done) {
        reject_done_with_open_subtasks(plan, task_id)?;
    }
    if matches!(next, TaskStatus::Paused) {
        return Err(PlanError::TaskPausedSystemOnly);
    }
    Ok(())
}

pub mod agent_lifecycle;
pub mod goal_ops;
#[cfg(test)]
mod in_progress_executor_tests;
pub mod plan_ops;
pub mod task_ops;
#[cfg(test)]
mod team_close_transaction_tests;
mod team_plan_create;
mod team_plan_detach;
