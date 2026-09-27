use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;

use super::readback::pending_after_verified;
use super::state::{dependency_waits, DependencyWait, TaskPresentation};
use super::{NewTask, PlanError, PlanStore};
use crate::plan::{
    OrchestratorRef, Plan, PlanMode, PlanStatus, Section, Task, TaskStatus, INBOX_PLAN_ID,
    INBOX_PLAN_TITLE,
};

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
    pub task: Task,
    pub section_titles: Vec<String>,
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

fn canonical_project(project_path: &str) -> String {
    super::paths::project_root(Path::new(project_path))
        .display()
        .to_string()
}

fn is_active_task(status: &TaskStatus) -> bool {
    status.counts_as_open_work()
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

fn task_sections(plan: &Plan, task: &Task) -> Vec<TaskSection> {
    let ids = task.section_ids.iter().map(String::as_str).collect();
    let mut out = vec![];
    collect_sections(&plan.sections, &ids, &mut out);
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

fn plan_in_scope(filter: &TaskListFilter, plan: &Plan) -> bool {
    if plan.status == PlanStatus::Archived {
        return false;
    }
    if filter
        .project_path
        .as_deref()
        .is_some_and(|project| plan.project_path != project)
    {
        return false;
    }
    !filter
        .plan_id
        .as_deref()
        .is_some_and(|id| plan.id.as_str() != id)
}

fn empty_reason(filter: &TaskListFilter, plans: &[Plan]) -> TaskListEmptyReason {
    if let Some(plan_id) = filter.plan_id.as_deref() {
        let Some(plan) = plans.iter().find(|plan| plan.id == plan_id) else {
            return TaskListEmptyReason::PlanNotFound {
                plan_id: plan_id.to_string(),
            };
        };
        if plan.status == PlanStatus::Archived {
            return TaskListEmptyReason::PlanArchived {
                plan_id: plan_id.to_string(),
            };
        }
        if let Some(project) = filter.project_path.as_deref() {
            if plan.project_path != project {
                return TaskListEmptyReason::PlanOutsideProject {
                    plan_id: plan_id.to_string(),
                    plan_project: plan.project_path.clone(),
                    requested_project: project.to_string(),
                };
            }
        }
    }
    let scoped: Vec<&Plan> = plans
        .iter()
        .filter(|plan| plan_in_scope(filter, plan))
        .collect();
    if scoped.is_empty() {
        return TaskListEmptyReason::NoPlansInScope {
            requested_project: filter.project_path.clone(),
        };
    }
    if scoped.iter().all(|plan| plan.tasks.is_empty()) {
        return TaskListEmptyReason::NoTasksInScope {
            plan_id: filter.plan_id.clone(),
        };
    }
    if filter.active_only {
        let relaxed = TaskListFilter {
            active_only: false,
            ..filter.clone()
        };
        let hidden = scoped
            .iter()
            .copied()
            .flat_map(|plan| plan.tasks.iter())
            .filter(|task| include_task(&relaxed, task))
            .count();
        if hidden > 0 {
            return TaskListEmptyReason::AllTasksInactive { hidden };
        }
    }
    TaskListEmptyReason::NoMatchForFilters
}

fn include_task(filter: &TaskListFilter, task: &Task) -> bool {
    if filter.active_only && !is_active_task(&task.status) {
        return false;
    }
    if filter.unassigned && task.assigned_tab_id.is_some() {
        return false;
    }
    if filter
        .status
        .as_ref()
        .is_some_and(|status| &task.status != status)
    {
        return false;
    }
    if !filter.mine_tabs.is_empty() {
        let Some(assigned) = task.assigned_tab_id.as_deref() else {
            return false;
        };
        if !filter.mine_tabs.contains(assigned) {
            return false;
        }
    }
    true
}

impl PlanStore {
    pub fn ensure_inbox_plan(
        &self,
        project_path: &str,
        owner: OrchestratorRef,
        now_ms: u64,
    ) -> Result<Plan, PlanError> {
        let project_path = canonical_project(project_path);
        if let Some(plan) = self.get(INBOX_PLAN_ID) {
            if plan.project_path == project_path {
                return Ok(plan);
            }
        }
        let mut plan = Plan::new(INBOX_PLAN_TITLE.into(), project_path, owner);
        plan.id = INBOX_PLAN_ID.into();
        plan.status = PlanStatus::Archived;
        self.save(&mut plan, now_ms)
            .map_err(|e| PlanError::Io(e.to_string()))?;
        Ok(plan)
    }

    pub fn add_inbox_task(
        &self,
        project_path: &str,
        owner: OrchestratorRef,
        new: NewTask,
        now_ms: u64,
    ) -> Result<(String, String), PlanError> {
        let plan = self.ensure_inbox_plan(project_path, owner, now_ms)?;
        let task_id = self.add_task(&plan.id, new, now_ms)?;
        Ok((plan.id, task_id))
    }

    pub fn list_tasks(&self, filter: TaskListFilter) -> Vec<TaskSummary> {
        self.list_tasks_with_reason(filter).tasks
    }

    pub fn list_tasks_with_reason(&self, mut filter: TaskListFilter) -> TaskListResult {
        filter.project_path = filter.project_path.map(|p| canonical_project(&p));
        let plans = self.list_full();
        let mut tasks = vec![];
        for plan in plans.iter().filter(|plan| plan_in_scope(&filter, plan)) {
            for task in &plan.tasks {
                if !include_task(&filter, task) {
                    continue;
                }
                let sections = task_sections(plan, task);
                tasks.push(TaskSummary {
                    plan_id: plan.id.clone(),
                    plan_title: plan.title.clone(),
                    plan_owner: plan.owner.clone(),
                    display_key: task.display_key(),
                    task: task.clone(),
                    section_titles: sections.into_iter().map(|s| s.title).collect(),
                });
            }
        }
        tasks.sort_by(|a, b| {
            a.plan_title
                .cmp(&b.plan_title)
                .then_with(|| a.task.name.cmp(&b.task.name))
        });
        let empty_reason = tasks.is_empty().then(|| empty_reason(&filter, &plans));
        TaskListResult {
            tasks,
            empty_reason,
        }
    }

    pub fn show_task(&self, task_id: &str) -> Option<TaskDetail> {
        for plan in self.list_full() {
            if plan.status == PlanStatus::Archived {
                continue;
            }
            let Some(task) = crate::plan::task_key::find_task(&plan.tasks, task_id) else {
                continue;
            };
            let parent = task.parent_task_id.as_ref().and_then(|parent_id| {
                plan.tasks
                    .iter()
                    .find(|candidate| candidate.id == *parent_id)
                    .map(task_label)
            });
            let dependency_waits = dependency_waits(&plan, task);
            return Some(TaskDetail {
                plan_id: plan.id.clone(),
                plan_title: plan.title.clone(),
                plan_owner: plan.owner.clone(),
                display_key: task.display_key(),
                sections: task_sections(&plan, task),
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
            });
        }
        None
    }

    pub fn set_task_assignment(
        &self,
        task_id: &str,
        tab_id: Option<&str>,
        taken_by_role: Option<&str>,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        let Some((plan_id, canonical_task_id)) = self.list_full().into_iter().find_map(|plan| {
            if plan.status == PlanStatus::Archived {
                return None;
            }
            crate::plan::task_key::find_task(&plan.tasks, task_id)
                .map(|task| (plan.id, task.id.clone()))
        }) else {
            return Err(PlanError::TaskNotFound);
        };
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
