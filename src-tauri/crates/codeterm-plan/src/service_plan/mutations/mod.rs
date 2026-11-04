use super::lane_metadata::{apply_lane_update, normalize_lane_metadata};
use super::{
    qa, tag_model, task_attempts, MutationWriterTurn, NewTask, PlanError, PlanMatch, PlanStore,
    TaskLaneUpdate, TaskMatch,
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

fn section_matches(section: &Section, selector: &str) -> bool {
    section.id == selector || section.handle.as_deref() == Some(selector)
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
