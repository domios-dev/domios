use codeterm_types::{plan::PlanStatus, team::Team};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, ts_rs::TS)]
pub struct TeamMissionControlQuery {
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, ts_rs::TS)]
pub struct TeamMissionControlEvent {
    pub id: String,
    pub plan_id: String,
    pub plan_title: String,
    pub ts: u64,
    pub last_ts: u64,
    pub count: usize,
    pub kind: String,
    pub author: String,
    pub author_tab_id: Option<String>,
    pub task_id: Option<String>,
    pub task_name: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[ts(
        as = "Option<Vec<crate::service_plan::state::mission_control::LinkedEventTask>>",
        optional
    )]
    pub linked_tasks: Vec<crate::service_plan::state::mission_control::LinkedEventTask>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, ts_rs::TS)]
pub struct TeamMissionControlState {
    pub team_id: String,
    pub team: Team,
    pub active_plan: Option<TeamPlanSummary>,
    pub archived_plans: Vec<TeamPlanSummary>,
    pub events: Vec<TeamMissionControlEvent>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, ts_rs::TS)]
pub struct TeamPlanSummary {
    pub id: String,
    pub title: String,
    pub team_id: Option<String>,
    pub status: PlanStatus,
    pub mode: codeterm_types::plan::PlanMode,
    pub planning_state: codeterm_types::plan::PlanningState,
    pub goal: Option<TeamPlanGoal>,
    pub updated_at: u64,
    pub task_counts: crate::service_plan::state::TaskCounts,
}

#[derive(Debug, Clone, Serialize, PartialEq, ts_rs::TS)]
pub struct TeamPlanGoal {
    pub vector: String,
}
