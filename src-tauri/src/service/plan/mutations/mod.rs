use super::lane_metadata::{apply_lane_update, normalize_lane_metadata};
use super::readback::pending_after_verified;
use super::{
    qa, section_links, tag_model, task_attempts, NewTask, OpenSubtask, PlanError, PlanStore,
    TaskLaneUpdate,
};
use crate::plan::{
    AgentRef, Goal, Milestone, MilestoneKind, OrchestratorRef, Plan, PlanMode, PlanStatus,
    PlannerRef, PlanningAuthor, PlanningState, PlanningTransition, Section, TagFacet, Task,
    TaskGroup, TaskPriority, TaskStatus, TransferRecord,
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

fn section_ids_contains(sections: &[Section], id: &str) -> bool {
    sections
        .iter()
        .any(|s| s.id == id || section_ids_contains(&s.subsections, id))
}

pub(super) fn push_subsection(
    sections: &mut Vec<Section>,
    parent_id: &str,
    child: Section,
) -> Option<Section> {
    for s in sections.iter_mut() {
        if s.id == parent_id {
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
    if let Some(pos) = sections.iter().position(|s| s.id == id) {
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

pub(super) fn reject_done_with_open_subtasks(plan: &Plan, task_id: &str) -> Result<(), PlanError> {
    let subtasks = open_subtasks(plan, task_id);
    if subtasks.is_empty() {
        Ok(())
    } else {
        Err(PlanError::OpenSubtasks { subtasks })
    }
}

pub(super) fn validate_task_status_transition(
    plan: &Plan,
    task_id: &str,
    current: &TaskStatus,
    next: &TaskStatus,
    incoming_label: Option<Option<&str>>,
) -> Result<(), PlanError> {
    if current == next {
        return Ok(());
    }
    let invariants_on = std::env::var("CODETERM_STATE_INVARIANTS").map_or(true, |v| v != "off");
    if invariants_on {
        let task = plan.tasks.iter().find(|t| t.id == task_id);
        if matches!(next, TaskStatus::InProgress) {
            let has_pane = task
                .and_then(|t| t.assigned_pane_id.as_deref().or(t.lane.pane_id.as_deref()))
                .is_some_and(|p| !p.trim().is_empty());
            if !has_pane {
                return Err(PlanError::InProgressRequiresPane);
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
    if matches!(next, TaskStatus::Done) {
        reject_done_with_open_subtasks(plan, task_id)?;
    }
    if matches!((current, next), (TaskStatus::InReview, TaskStatus::Done)) {
        return Err(PlanError::TransitionDenied {
            from: current.clone(),
            to: next.clone(),
        });
    }
    // Parking must carry a reason + stride: force `plan task park`, never a bare status set.
    if matches!(next, TaskStatus::Parked) {
        return Err(PlanError::ParkRequiresReason);
    }
    Ok(())
}

mod plan_ops;
mod task_ops;
