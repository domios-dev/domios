use super::PlanStore;
use crate::plan::{Plan, PlanMode, PlanStatus};
use serde_json::{json, Value};

fn snapshot(plan: &Plan, query: Option<&str>, remaining: &mut usize) -> Value {
    let tasks: Vec<_> = plan.tasks.iter().filter(|task| {
        query.is_none_or(|query| format!("{} {} {}", task.id, task.display_key(), task.name).to_lowercase().contains(query))
    }).take(*remaining).map(|task| {
        let milestones: Vec<_> = task.milestones.iter().map(|stride| json!({
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
            "acceptance_criteria": task.acceptance_criteria.iter().map(|_| "").collect::<Vec<_>>()
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
    pub fn chat_snapshot(
        &self,
        project: Option<&str>,
        team: Option<&str>,
        query: Option<&str>,
    ) -> Vec<Value> {
        let project = project.map(|path| {
            super::paths::project_root(std::path::Path::new(path))
                .display()
                .to_string()
        });
        let query = query.map(str::to_lowercase);
        let mut remaining = if query.is_some() { 50 } else { usize::MAX };
        let mut result = Vec::new();
        // The index filters on stored columns, so plans outside project, team and status never load.
        let mut ids: Vec<String> = self
            .index
            .read()
            .values()
            .filter(|row| {
                project
                    .as_ref()
                    .is_none_or(|project| *project == row.project_path)
                    && team.is_none_or(|team| row.team_id.as_deref().unwrap_or("") == team)
                    && (query.is_none() || row.status == PlanStatus::Active)
            })
            .map(|row| row.id.clone())
            .collect();
        ids.sort();
        for id in ids {
            let Ok(plan) = self.plan_arc(&id) else {
                continue;
            };
            let plan = plan.lock();
            if query.is_some() && plan.mode == PlanMode::Complete {
                continue;
            }
            let value = snapshot(&plan, query.as_deref(), &mut remaining);
            if query.as_ref().is_none_or(|query| {
                plan.title.to_lowercase().contains(query)
                    || plan.id.contains(query)
                    || value["tasks"]
                        .as_array()
                        .is_some_and(|tasks| !tasks.is_empty())
            }) {
                result.push(value);
            }
            if query.is_some() && result.len() >= 50 {
                break;
            }
        }
        result
    }
}

#[cfg(test)]
mod tests;
