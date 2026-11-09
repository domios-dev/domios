use crate::plan::{
    AgentRef, ElementCanvas, OrchestratorHeartbeat, OrchestratorRef, PlanCanvas, PlanMode,
    PlanStatus, PlannerRef, TaskLaneMetadata, TaskStatus,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, ts_rs::TS)]
pub struct ChatPlanRead {
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
    #[serde(default)]
    pub tasks: Vec<ChatTaskRead>,
    #[serde(default)]
    pub sections: Vec<super::super::ui_projections::views::PlanSectionView>,
    #[serde(default)]
    pub milestone_kinds: Vec<crate::plan::MilestoneKind>,
    #[serde(default)]
    pub loops: Vec<crate::plan::LoopSpec>,
    #[serde(default)]
    pub elements: Vec<crate::plan::CanvasElement>,
    #[serde(default)]
    pub owner_history: Vec<crate::plan::TransferRecord>,
    #[serde(default)]
    pub revision: String,
    #[serde(skip_deserializing)]
    pub task_counts: super::super::state::TaskCounts,
}

#[derive(Debug, Deserialize, Serialize, ts_rs::TS)]
pub struct ChatTaskRead {
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
    #[serde(default)]
    pub model_rationale: Option<String>,
    #[serde(default)]
    pub intent: Option<String>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub criteria_count: usize,
    #[serde(default)]
    pub milestone_count: usize,
}

#[derive(Debug, Deserialize, Serialize, ts_rs::TS)]
pub struct ChatMilestoneRead {
    id: String,
    ts: u64,
    kind: String,
    from: AgentRef,
    loop_id: Option<String>,
    iteration: Option<u32>,
    canvas: ElementCanvas,
    #[serde(default)]
    text: String,
    #[serde(default)]
    links: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, ts_rs::TS)]
pub struct ChatAttemptRead {
    id: String,
    executor_tab_id: String,
    role: Option<String>,
    model: Option<String>,
    session_model: Option<String>,
    assigned_at: Option<u64>,
}

pub(in crate::service_plan) fn snapshot(
    mut plan: ChatPlanRead,
    selected_tasks: Vec<ChatTaskRead>,
    remaining: &mut usize,
) -> ChatPlanRead {
    plan.tasks = selected_tasks
        .into_iter()
        .take(*remaining)
        .map(|mut task| {
            task.milestones = task.milestones.into_iter().rev().take(1).collect();
            task.execution_attempts = task
                .execution_attempts
                .into_iter()
                .max_by_key(|attempt| attempt.assigned_at)
                .into_iter()
                .collect();
            task.key = Some(
                task.key
                    .unwrap_or_else(|| crate::plan::task_key::fallback_key(&task.id)),
            );
            task
        })
        .collect();
    *remaining = remaining.saturating_sub(plan.tasks.len());
    plan
}
