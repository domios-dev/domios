use serde::Serialize;

use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::AgentRole;
use crate::plan::{Plan, PlanStatus, Task, TaskStatus};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TeamCloseBusyTask {
    pub plan_id: String,
    pub task_id: String,
    pub name: String,
    pub tab_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TeamCloseBusyWorker {
    pub tab_id: String,
    pub title: String,
    pub working: bool,
    pub task_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TeamCloseFlaggedPlan {
    pub plan_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TeamCloseBusy {
    pub(crate) workers: Vec<TeamCloseBusyWorker>,
    pub(crate) tasks: Vec<TeamCloseBusyTask>,
}

impl TeamCloseBusy {
    pub(crate) fn is_empty(&self) -> bool {
        self.workers.is_empty() && self.tasks.is_empty()
    }

    pub(crate) fn refusal(&self, team_id: &str) -> String {
        let workers = self.workers.iter().map(|worker| {
            let activity = if worker.working {
                "working"
            } else {
                "holding work"
            };
            format!("Worker {} ({}) is {activity}", worker.tab_id, worker.title)
        });
        let tasks = self.tasks.iter().map(|task| {
            format!(
                "task {} \"{}\" of plan {} is in_progress on tab {}",
                task.task_id,
                task.name,
                task.plan_id,
                task.tab_id.as_deref().unwrap_or("<none>")
            )
        });
        format!(
            "{}: Team {team_id} has unfinished work: {}. Rerun with --force to close it anyway; \
             the output then names every killed Worker and task, and the plan stays active and flagged.",
            crate::service::TEAM_CLOSE_BUSY_REFUSAL_PREFIX,
            workers.chain(tasks).collect::<Vec<_>>().join("; ")
        )
    }
}

fn open_executor(task: &Task, members: &[String]) -> Option<String> {
    task.assigned_tab_id
        .iter()
        .chain(task.lane.tab_id.iter())
        .chain(
            task.execution_attempts
                .iter()
                .filter(|attempt| attempt.closed_at.is_none())
                .map(|attempt| &attempt.executor_tab_id),
        )
        .find(|tab_id| members.contains(tab_id))
        .cloned()
}

/// Work a Team close would destroy: in-progress Team tasks and Worker members
/// that are working or executing one of them.
pub(crate) fn team_close_busy(
    mux: &MultiplexerState,
    plans: &[Plan],
    team_id: &str,
    member_tab_ids: &[String],
) -> TeamCloseBusy {
    let mut tasks = Vec::new();
    for plan in plans
        .iter()
        .filter(|plan| plan.status == PlanStatus::Active)
    {
        let team_plan = plan.team_id.as_deref() == Some(team_id);
        for task in plan
            .tasks
            .iter()
            .filter(|task| task.status == TaskStatus::InProgress)
        {
            let executor = open_executor(task, member_tab_ids);
            if team_plan || executor.is_some() {
                tasks.push(TeamCloseBusyTask {
                    plan_id: plan.id.clone(),
                    task_id: task.id.clone(),
                    name: task.name.clone(),
                    tab_id: executor.or_else(|| task.assigned_tab_id.clone()),
                });
            }
        }
    }
    let workers = member_tab_ids
        .iter()
        .filter_map(|tab_id| {
            let tab = mux.get_tab(tab_id)?;
            if tab.role != Some(AgentRole::Worker) {
                return None;
            }
            let working = tab.agent_state.as_ref().is_some_and(|state| {
                matches!(
                    state.state,
                    crate::agent::offline_state_classifier::TimelineState::Working
                )
            });
            let task_ids: Vec<String> = tasks
                .iter()
                .filter(|task| task.tab_id.as_deref() == Some(tab_id.as_str()))
                .map(|task| task.task_id.clone())
                .collect();
            (working || !task_ids.is_empty()).then(|| TeamCloseBusyWorker {
                tab_id: tab_id.clone(),
                title: tab.title.clone(),
                working,
                task_ids,
            })
        })
        .collect();
    TeamCloseBusy { workers, tasks }
}
