use crate::plan::{
    AgentRef, ElementCanvas, OrchestratorHeartbeat, OrchestratorRef, PlanCanvas, PlanMode,
    PlanStatus, PlannerRef, TaskLaneMetadata, TaskStatus,
};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
pub(in crate::service_plan) struct ChatPlanRead {
    pub id: String,
    pub title: String,
    pub status: PlanStatus,
    pub mode: PlanMode,
    pub owner: OrchestratorRef,
    pub planner: Option<PlannerRef>,
    pub team_id: Option<String>,
    pub project_path: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub canvas: PlanCanvas,
    #[serde(default)]
    pub heartbeat: OrchestratorHeartbeat,
}

#[derive(Deserialize)]
pub(in crate::service_plan) struct ChatTaskRead {
    pub id: String,
    pub key: Option<String>,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub status: TaskStatus,
    pub status_label: Option<String>,
    pub model: Option<String>,
    pub assigned_tab_id: Option<String>,
    pub lane: TaskLaneMetadata,
    pub parent_task_id: Option<String>,
    pub peer: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub status_changed_at: u64,
    pub section_ids: Vec<String>,
    pub canvas: ElementCanvas,
    pub acceptance_criteria: Vec<String>,
    pub milestones: Vec<ChatMilestoneRead>,
    pub execution_attempts: Vec<ChatAttemptRead>,
}

#[derive(Deserialize)]
pub(in crate::service_plan) struct ChatMilestoneRead {
    id: String,
    ts: u64,
    kind: String,
    from: AgentRef,
    loop_id: Option<String>,
    iteration: Option<u32>,
    canvas: ElementCanvas,
}

#[derive(Deserialize)]
pub(in crate::service_plan) struct ChatAttemptRead {
    id: String,
    executor_tab_id: String,
    role: Option<String>,
    model: Option<String>,
    session_model: Option<String>,
    assigned_at: Option<u64>,
}

pub(in crate::service_plan) fn snapshot(
    plan: &ChatPlanRead,
    selected_tasks: &[ChatTaskRead],
    remaining: &mut usize,
) -> Value {
    let tasks: Vec<_> = selected_tasks.iter().take(*remaining).map(|task| {
        let milestones: Vec<_> = task.milestones.iter().rev().take(1).map(|stride| json!({
            "id": stride.id, "ts": stride.ts, "kind": stride.kind, "from": stride.from,
            "text": "", "links": [], "loop_id": stride.loop_id, "iteration": stride.iteration, "canvas": stride.canvas
        })).collect();
        let attempts: Vec<_> = task.execution_attempts.iter().max_by_key(|attempt| attempt.assigned_at).map(|attempt| json!({
            "id": attempt.id, "executor_tab_id": attempt.executor_tab_id, "role": attempt.role,
            "model": attempt.model, "session_model": attempt.session_model, "assigned_at": attempt.assigned_at
        })).into_iter().collect();
        json!({
            "id": task.id, "key": task.key.clone().unwrap_or_else(|| crate::plan::task_key::fallback_key(&task.id)), "name": task.name,
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
