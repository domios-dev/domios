use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

use super::readback::pending_after_verified;
use super::state::{dependency_waits, DependencyWait, TaskPresentation};
use super::{PlanError, PlanMatch, PlanStore, TaskMatch};
use crate::plan::{OrchestratorRef, Plan, PlanMode, Section, Task, TaskStatus};

#[derive(Debug, Clone, Default)]
pub struct TaskListFilter {
    pub project_path: Option<String>,
    pub active_only: bool,
    pub mine_tabs: HashSet<String>,
    pub unassigned: bool,
    pub plan_id: Option<String>,
    pub status: Option<TaskStatus>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskSummary {
    pub plan_id: String,
    pub plan_title: String,
    pub plan_owner: OrchestratorRef,
    pub display_key: String,
    pub task: TaskListTask,
    pub section_titles: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TaskListTask {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub status: TaskStatus,
    pub assigned_tab_id: Option<String>,
    pub key: Option<String>,
    pub handle: Option<String>,
    pub parent_task_id: Option<String>,
    pub pause_reason: Option<String>,
    pub status_label: Option<String>,
    #[serde(default)]
    pub section_ids: Vec<String>,
    #[serde(default)]
    pub external_refs: Vec<crate::plan::ExternalRef>,
    #[serde(default)]
    pub lane: TaskListLane,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TaskListLane {
    pub tab_id: Option<String>,
    pub workspace_id: Option<String>,
    pub branch: Option<String>,
    pub cleanup_status: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskDetail {
    pub plan_id: String,
    pub plan_title: String,
    pub plan_owner: OrchestratorRef,
    pub sections: Vec<TaskSection>,
    pub parent: Option<TaskLabel>,
    pub siblings: Vec<TaskLabel>,
    pub subtasks: Vec<TaskLabel>,
    pub presentation: TaskPresentation,
    pub dependency_waits: Vec<DependencyWait>,
    pub display_key: String,
    pub task: Task,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskSection {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TaskLabel {
    pub id: String,
    pub display_key: String,
    pub name: String,
    pub status: TaskStatus,
}

/// Why a task listing came back empty. Only the store can tell these apart:
/// project identity is worktree-aware here and nowhere else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskListEmptyReason {
    PlanNotFound {
        plan_id: String,
    },
    PlanArchived {
        plan_id: String,
    },
    PlanOutsideProject {
        plan_id: String,
        plan_project: String,
        requested_project: String,
    },
    NoPlansInScope {
        requested_project: Option<String>,
    },
    NoTasksInScope {
        plan_id: Option<String>,
    },
    AllTasksInactive {
        hidden: usize,
    },
    NoMatchForFilters,
}

/// The wire shape of `task.list` for every transport: serialize this whole
/// struct, never hand-build `{ "tasks": ... }`, or the reason key drifts apart.
#[derive(Debug, Clone, Serialize)]
pub struct TaskListResult {
    pub tasks: Vec<TaskSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub empty_reason: Option<TaskListEmptyReason>,
}

pub(super) fn canonical_project(project_path: &str) -> String {
    super::paths::project_root(Path::new(project_path))
        .display()
        .to_string()
}

fn collect_sections(sections: &[Section], ids: &HashSet<&str>, out: &mut Vec<TaskSection>) {
    for section in sections {
        if ids.contains(section.id.as_str()) {
            out.push(TaskSection {
                id: section.id.clone(),
                title: section.title.clone(),
            });
        }
        collect_sections(&section.subsections, ids, out);
    }
}

fn task_sections(sections: &[Section], task: &Task) -> Vec<TaskSection> {
    let ids = task.section_ids.iter().map(String::as_str).collect();
    let mut out = vec![];
    collect_sections(sections, &ids, &mut out);
    out
}

fn task_label(task: &Task) -> TaskLabel {
    TaskLabel {
        id: task.id.clone(),
        display_key: task.display_key(),
        name: task.name.clone(),
        status: task.status.clone(),
    }
}

/// Unarchived plans holding a task this reference may name; callers confirm with `find_task`.
fn task_ref_candidates(task_ref: &str) -> PlanMatch {
    PlanMatch::active().and(TaskMatch::reference(task_ref))
}

impl PlanStore {
    pub fn list_tasks(&self, filter: TaskListFilter) -> Result<Vec<TaskSummary>, PlanError> {
        self.list_tasks_with_reason(filter)
            .map(|result| result.tasks)
    }

    pub fn list_tasks_with_reason(
        &self,
        mut filter: TaskListFilter,
    ) -> Result<TaskListResult, PlanError> {
        filter.project_path = filter.project_path.map(|p| canonical_project(&p));
        self.database.task_list_snapshot(&filter)
    }

    pub fn show_task(&self, task_id: &str) -> Option<TaskDetail> {
        self.plans_matching(&task_ref_candidates(task_id))
            .iter()
            .find_map(|plan| detail_from_plan(plan, task_id))
    }

    pub fn show_task_window(&self, task_id: &str) -> Result<TaskDetail, PlanError> {
        let plan = self
            .database
            .task_detail_snapshot(&task_ref_candidates(task_id), task_id, true)?
            .ok_or(PlanError::TaskNotFound)?;
        detail_from_plan(&plan, task_id).ok_or(PlanError::TaskNotFound)
    }

    pub fn set_task_assignment(
        &self,
        task_id: &str,
        tab_id: Option<&str>,
        taken_by_role: Option<&str>,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        let target = self.task_authority_target_projection(task_id)?;
        let plan_id = target.plan_id;
        let canonical_task_id = target.task_id;
        let write = self.with_plan(&plan_id, now_ms, |p| {
            let task_index = p
                .tasks
                .iter()
                .position(|task| task.id == canonical_task_id)
                .ok_or(PlanError::TaskNotFound)?;
            if let Some(tab) = tab_id.filter(|_| p.tasks[task_index].status == TaskStatus::Pending)
            {
                let lane = super::TaskLaneUpdate {
                    tab_id: Some(Some(tab.to_string())),
                    ..Default::default()
                };
                super::mutations::validate_task_status_transition_with_lane(
                    p,
                    &canonical_task_id,
                    &TaskStatus::Pending,
                    &TaskStatus::InProgress,
                    None,
                    Some(&lane),
                )?;
            }
            let task = &mut p.tasks[task_index];
            task.assigned_tab_id = tab_id.map(str::to_string);
            task.taken_by_role = taken_by_role.map(str::to_string);
            if let Some(tab) = tab_id {
                let attempt = super::task_attempts::new_assignment_attempt(
                    task,
                    tab,
                    None,
                    taken_by_role,
                    now_ms,
                );
                task.execution_attempts.push(attempt);
            }
            // Binding an executor tab IS the in_progress precondition — taking or dispatching a task starts it, so a live worker never leaves the board stuck at pending while it codes.
            if tab_id.is_some() && task.status == TaskStatus::Pending {
                task.status = TaskStatus::InProgress;
                task.status_changed_at = now_ms;
                p.mode = PlanMode::Execution;
            }
            if tab_id.is_none() {
                task.peer = None;
            }
            task.updated_at = now_ms;
            Ok(())
        });
        pending_after_verified(
            write,
            self.verify_task_assignment(&plan_id, &canonical_task_id, tab_id, taken_by_role),
            Some(canonical_task_id),
        )?;
        Ok(plan_id)
    }
}

fn detail_from_plan(plan: &Plan, task_id: &str) -> Option<TaskDetail> {
    let task = crate::plan::task_key::find_task(&plan.tasks, task_id)?;
    let parent = task.parent_task_id.as_ref().and_then(|parent_id| {
        plan.tasks
            .iter()
            .find(|candidate| candidate.id == *parent_id)
            .map(task_label)
    });
    let dependency_waits = dependency_waits(&plan, task);
    Some(TaskDetail {
        plan_id: plan.id.clone(),
        plan_title: plan.title.clone(),
        plan_owner: plan.owner.clone(),
        display_key: task.display_key(),
        sections: task_sections(&plan.sections, task),
        parent,
        siblings: plan
            .tasks
            .iter()
            .filter(|candidate| {
                candidate.id != task.id && candidate.parent_task_id == task.parent_task_id
            })
            .map(task_label)
            .collect(),
        subtasks: plan
            .tasks
            .iter()
            .filter(|candidate| candidate.parent_task_id.as_deref() == Some(&task.id))
            .map(task_label)
            .collect(),
        presentation: super::state::task_projection(&plan, task).presentation,
        dependency_waits,
        task: task.clone(),
    })
}
