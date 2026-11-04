use super::lane_metadata::{apply_lane_update, normalize_lane_metadata};
use super::{
    qa, tag_model, task_attempts, MutationWriterTurn, NewTask, OpenSubtask, PlanError, PlanMatch,
    PlanStore, TaskLaneUpdate, TaskMatch,
};
use crate::plan::{
    AgentRef, Goal, Milestone, MilestoneKind, OrchestratorRef, Plan, PlanMode, PlanStatus,
    PlannerRef, PlanningAuthor, PlanningState, ReviewEvidence, ReviewVerdict, Section, TagFacet,
    Task, TaskGroup, TaskStatus, TransferRecord,
};

mod plan_writer;
mod subtasks;
mod task_add;
mod task_files;

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

pub(super) fn ensure_group_declared(plan: &mut Plan, key: Option<&str>) {
    let Some(key) = key.filter(|key| !key.trim().is_empty()) else {
        return;
    };
    if !plan.task_groups.iter().any(|group| group.key == key) {
        plan.task_groups.push(TaskGroup {
            key: key.to_string(),
            label: key.to_string(),
            color: None,
        });
    }
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

pub(super) fn has_section(sections: &[Section], id: &str) -> bool {
    sections.iter().any(|section| {
        section.id == id
            || section.handle.as_deref() == Some(id)
            || has_section(&section.subsections, id)
    })
}

fn parse_plan_mode(raw: &str) -> Result<PlanMode, PlanError> {
    serde_json::from_value(serde_json::Value::String(raw.into())).map_err(|_| PlanError::BadMode)
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

pub(super) fn reject_terminal_with_open_subtasks(
    plan: &Plan,
    task_id: &str,
) -> Result<(), PlanError> {
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
    reject_terminal_with_open_subtasks(plan, &task_id)?;
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
    let task = plan.tasks.iter().find(|task| task.id == task_id);
    let task_level_tab = task
        .and_then(|task| {
            task.assigned_tab_id
                .as_deref()
                .or(task.lane.tab_id.as_deref())
        })
        .is_some_and(|tab| !tab.trim().is_empty());
    let has_tab = match executor {
        Some(InProgressExecutor::LaneAssignment(update)) => match update.tab_id.as_ref() {
            Some(tab) => tab.as_deref().is_some_and(|tab| !tab.trim().is_empty()),
            None => task_level_tab,
        },
        Some(InProgressExecutor::CompetitiveAttempt(attempt_id)) => {
            task.is_some_and(|task| live_competitive_executor(task, attempt_id))
        }
        None => task_level_tab,
    };
    status_transition::validate(
        task,
        current,
        next,
        incoming_label,
        has_tab,
        |task| super::review_gate::validate_dispatch_review_capacity(plan, task),
        || reject_terminal_with_open_subtasks(plan, task_id),
    )
}

pub(in crate::service_plan) mod status_transition;

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
