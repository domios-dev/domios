use crate::plan::{LoopStatus, OrchestratorRef, PlanStatus, Task, TaskStatus};
use crate::service_plan::task_queries::TaskSection;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Deserialize, Serialize, ts_rs::TS)]
pub struct PlanTaskBoardSummary {
    pub id: String,
    pub position: i64,
    pub name: String,
    pub status: TaskStatus,
    pub created_at: u64,
    pub updated_at: u64,
    pub status_changed_at: u64,
    pub activity_at: u64,
    pub handle: Option<String>,
    pub key: Option<String>,
    pub assigned_tab_id: Option<String>,
    pub lane: PlanTaskBoardLane,
    pub taken_by_role: Option<String>,
    pub pause_reason: Option<String>,
    pub model: Option<String>,
    pub peer: Option<String>,
    pub review_stage: Option<String>,
    pub qa_state: Option<String>,
    pub status_label: Option<String>,
    pub qa_queue_position: u64,
    pub locator: Option<PlanTaskBoardLocator>,
    pub latest_note: Option<String>,
    pub historical_executor: Option<PlanTaskHistoricalExecutor>,
    pub competition_badge: Option<PlanTaskCompetitionBadge>,
    pub loop_badge: Option<PlanTaskLoopBadge>,
}

#[derive(Debug, Deserialize, Serialize, ts_rs::TS)]
pub struct PlanTaskBoardLane {
    pub tab_id: Option<String>,
    pub branch: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct PlanTaskBoardLocator {
    pub group_key: Option<String>,
    pub group_label: Option<String>,
    pub group_color: Option<String>,
    pub section: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, ts_rs::TS)]
pub struct PlanTaskHistoricalExecutor {
    pub tab_id: Option<String>,
    pub tab_title: Option<String>,
    pub role: Option<String>,
    pub model: Option<String>,
    pub branch: Option<String>,
    pub report_status: Option<String>,
    pub outcome: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, ts_rs::TS)]
pub struct PlanTaskCompetitionAttemptSummary {
    pub tab_id: Option<String>,
    pub tab_title: Option<String>,
    pub model: Option<String>,
    pub competition_lane: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize, ts_rs::TS)]
pub struct PlanTaskCompetitionBadge {
    pub count: u64,
    pub live_count: u64,
    pub judge_hint: Option<String>,
    pub models: Vec<String>,
    pub live_attempts: Vec<PlanTaskCompetitionAttemptSummary>,
}

#[derive(Debug, Deserialize, Serialize, ts_rs::TS)]
pub struct PlanTaskLoopBadge {
    pub count: u64,
    pub budget: u64,
    pub status: LoopStatus,
    #[serde(rename = "stopWhen")]
    pub stop_when: Option<String>,
    pub warning: bool,
    pub tooltip: String,
    pub loops: Vec<PlanTaskLoopSummary>,
}

#[derive(Debug, Deserialize, Serialize, ts_rs::TS)]
pub struct PlanTaskLoopSummary {
    pub id: String,
    pub target: String,
    pub budget: u64,
    pub count: u64,
    pub stop_when: Option<String>,
    pub status: LoopStatus,
    pub stagnation: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize, ts_rs::TS)]
pub struct PlanTaskDetailContext {
    pub plan_id: String,
    pub title: String,
    pub status: PlanStatus,
    pub team_id: Option<String>,
    pub owner: OrchestratorRef,
    pub sections: Vec<TaskSection>,
    pub has_open_subtasks: bool,
}

#[derive(Debug, Serialize, ts_rs::TS)]
#[serde(untagged)]
pub enum PlanTaskHistoryItem {
    Milestone(crate::plan::Milestone),
    Attempt(crate::plan::TaskExecutionAttempt),
}

#[derive(Debug, Serialize, ts_rs::TS)]
pub struct PlanTaskHistoryPage {
    pub plan_id: String,
    pub task_id: String,
    pub revision: String,
    pub items: Vec<PlanTaskHistoryItem>,
    pub next_cursor: Option<i64>,
}

#[derive(Debug, Serialize, ts_rs::TS)]
pub struct PlanTaskHistoryCursors {
    pub milestones: Option<i64>,
    pub attempts: Option<i64>,
}

#[derive(Debug, Serialize, ts_rs::TS)]
pub struct PlanTaskDetail {
    pub revision: String,
    pub plan_id: String,
    pub task_id: String,
    pub task: Task,
    pub context: PlanTaskDetailContext,
    pub loop_badge: Option<PlanTaskLoopBadge>,
    pub history: PlanTaskHistoryCursors,
}

#[derive(Debug, Serialize, ts_rs::TS)]
pub struct PlanTaskBoardCursor {
    pub created_at: u64,
    pub position: i64,
    pub task_id: String,
}

#[derive(Debug, Serialize, ts_rs::TS)]
pub struct PlanTaskBoardColumnPage {
    pub status: String,
    pub items: Vec<PlanTaskBoardSummary>,
    pub next_cursor: Option<PlanTaskBoardCursor>,
}

#[derive(Debug, Serialize, ts_rs::TS)]
pub struct PlanTaskBoardCounts {
    pub pending: u64,
    pub in_progress: u64,
    pub in_review: u64,
    pub blocked: u64,
    pub done: u64,
    pub cancelled: u64,
    pub total: u64,
}

#[derive(Debug, Serialize, ts_rs::TS)]
pub struct PlanTaskBoardPage {
    pub revision: String,
    pub plan_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub counts: Option<PlanTaskBoardCounts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub columns: Option<BTreeMap<String, PlanTaskBoardColumnPage>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub column: Option<PlanTaskBoardColumnPage>,
}
