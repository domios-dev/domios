use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

use super::state::{DependencyWait, TaskPresentation};
use super::{PlanError, PlanStore};
use crate::plan::{OrchestratorRef, PlanMode, Task, TaskStatus};

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

    pub fn show_task_window(&self, task_id: &str) -> Result<TaskDetail, PlanError> {
        self.database
            .task_detail_projection(task_id)?
            .ok_or(PlanError::TaskNotFound)
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
        self.with_task_rows(
            &plan_id,
            &canonical_task_id,
            super::store_db::hot_write::TaskHistory::Core,
            now_ms,
            |edit| {
                if tab_id.is_some() && edit.task.status == TaskStatus::Pending {
                    edit.progression = Some((
                        TaskStatus::Pending,
                        tab_id.is_some_and(|tab| !tab.trim().is_empty()),
                    ));
                }
                let task = &mut edit.task;
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
                    edit.mode = PlanMode::Execution;
                }
                if tab_id.is_none() {
                    task.peer = None;
                }
                task.updated_at = now_ms;
                Ok(())
            },
        )?;
        Ok(plan_id)
    }
}

#[cfg(any(test, feature = "test-support"))]
mod fixture;
