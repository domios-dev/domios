use super::PlanStore;
use crate::plan::Plan;
use serde_json::{json, Value};

pub(super) fn snapshot(plan: &Plan, query: Option<&str>, remaining: &mut usize) -> Value {
    let tasks: Vec<_> = plan.tasks.iter().filter(|task| {
        query.is_none_or(|query| format!("{} {} {}", task.id, task.display_key(), task.name).to_lowercase().contains(query))
    }).take(*remaining).map(|task| {
        let milestones: Vec<_> = task.milestones.iter().rev().take(1).map(|stride| json!({
            "id": stride.id, "ts": stride.ts, "kind": stride.kind, "from": stride.from,
            "text": "", "links": [], "loop_id": stride.loop_id, "iteration": stride.iteration, "canvas": stride.canvas
        })).collect();
        let attempts: Vec<_> = task.execution_attempts.iter().max_by_key(|attempt| attempt.assigned_at).map(|attempt| json!({
            "id": attempt.id, "executor_tab_id": attempt.executor_tab_id, "role": attempt.role,
            "model": attempt.model, "session_model": attempt.session_model, "assigned_at": attempt.assigned_at
        })).into_iter().collect();
        json!({
            "id": task.id, "key": task.display_key(), "name": task.name,
            "type": task.kind, "status": task.status, "status_label": task.status_label,
            "model": task.model, "model_rationale": null, "intent": null,
            "assigned_tab_id": task.assigned_tab_id, "lane": task.lane,
            "parent_task_id": task.parent_task_id, "peer": task.peer,
            "created_at": task.created_at, "updated_at": task.updated_at,
            "status_changed_at": task.status_changed_at,
            "execution_attempts": attempts,
            "section_ids": task.section_ids, "body": "", "milestones": milestones,
            "canvas": task.canvas,
            "acceptance_criteria": task.acceptance_criteria.iter().take(1).map(|_| "").collect::<Vec<_>>()
        })
    }).collect();
    *remaining = remaining.saturating_sub(tasks.len());
    json!({
        "id": plan.id, "title": plan.title, "status": plan.status, "mode": plan.mode,
        "owner": plan.owner, "planner": plan.planner, "team_id": plan.team_id,
        "project_path": plan.project_path, "created_at": plan.created_at, "updated_at": plan.updated_at,
        "tasks": tasks, "sections": [], "milestone_kinds": [], "loops": [], "elements": [],
        "canvas": plan.canvas, "owner_history": [], "heartbeat": plan.heartbeat
    })
}

impl PlanStore {
    pub fn chat_snapshot_for_tab(
        &self,
        project: Option<&str>,
        team: Option<&str>,
        query: Option<&str>,
        tab: Option<&str>,
    ) -> Result<Vec<Value>, super::PlanError> {
        let project = project.map(|path| {
            super::paths::project_root(std::path::Path::new(path))
                .display()
                .to_string()
        });
        let query = query.map(str::to_lowercase);
        self.database
            .chat_projection(project.as_deref(), team, query.as_deref(), None, tab)
    }

    pub fn chat_snapshot(
        &self,
        project: Option<&str>,
        team: Option<&str>,
        query: Option<&str>,
    ) -> Result<Vec<Value>, super::PlanError> {
        self.chat_snapshot_for_tab(project, team, query, None)
    }
}

#[cfg(test)]
mod tests;
