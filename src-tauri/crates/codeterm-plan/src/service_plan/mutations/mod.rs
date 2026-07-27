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
    task.assigned_pane_id = None;
    task.lane.pane_id = None;
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

/// A dependency ref may name a task by full id or by handle (`--depends-on` accepts both).
fn resolve_dependency_ref(plan: &Plan, reference: &str) -> Result<String, PlanError> {
    if let Some(task) = plan.tasks.iter().find(|task| task.id == reference) {
        return Ok(task.id.clone());
    }
    if let Some(task) = plan
        .tasks
        .iter()
        .find(|task| task.handle.as_deref() == Some(reference))
    {
        return Ok(task.id.clone());
    }
    Err(PlanError::DependencyTaskNotFound {
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
    let assigned = task.assigned_pane_id.clone();
    crate::service_plan::task_attempts::mark_task_assignment_closed(
        task,
        assigned.as_deref(),
        now_ms,
    );
    clear_task_assignment(task);
    task.updated_at = now_ms;
    Ok(())
}

pub(super) fn validate_task_status_transition(
    plan: &Plan,
    task_id: &str,
    current: &TaskStatus,
    next: &TaskStatus,
    incoming_label: Option<Option<&str>>,
) -> Result<(), PlanError> {
    validate_task_status_transition_with_lane(plan, task_id, current, next, incoming_label, None)
}

pub(super) fn validate_task_status_transition_with_lane(
    plan: &Plan,
    task_id: &str,
    current: &TaskStatus,
    next: &TaskStatus,
    incoming_label: Option<Option<&str>>,
    lane: Option<&super::TaskLaneUpdate>,
) -> Result<(), PlanError> {
    let status_unchanged = current == next;
    let invariants_on = std::env::var("CODETERM_STATE_INVARIANTS").map_or(true, |v| v != "off");
    if invariants_on {
        let task = plan.tasks.iter().find(|t| t.id == task_id);
        if matches!(next, TaskStatus::InProgress) {
            let incoming_lane_pane = lane.and_then(|update| update.pane_id.as_ref());
            let has_pane = match incoming_lane_pane {
                Some(pane) => pane.as_deref().is_some_and(|p| !p.trim().is_empty()),
                None => task
                    .and_then(|t| t.assigned_pane_id.as_deref().or(t.lane.pane_id.as_deref()))
                    .is_some_and(|p| !p.trim().is_empty()),
            };
            if !has_pane {
                return Err(PlanError::InProgressRequiresPane);
            }
            // Only fresh dispatch (claiming new capacity) is throttled; a task
            // returning to in_progress after a review bounce already holds its
            // slot and must not get stuck re-queued behind its own review.
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
    if matches!(current, TaskStatus::InReview) {
        return Err(PlanError::TransitionDenied {
            from: current.clone(),
            to: next.clone(),
        });
    }
    if matches!(next, TaskStatus::Done) {
        reject_done_with_open_subtasks(plan, task_id)?;
    }
    // Pausing must carry a reason + stride: force `plan task pause`, never a bare status set.
    if matches!(next, TaskStatus::Paused) {
        return Err(PlanError::TaskPauseRequiresReason);
    }
    Ok(())
}

pub mod agent_lifecycle;
mod plan_ops;
mod task_ops;
#[cfg(test)]
mod team_close_transaction_tests;
mod team_plan_create;
mod team_plan_detach;
