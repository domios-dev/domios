use crate::plan::{
    AttachmentRef, ElementCanvas, MilestoneKind, OrchestratorRef, PlanCanvas, PlanMode, PlanStatus,
    PlannerRef, QaState, ReviewStage, SpecLink, TagFacet, TaskGroup, TaskLaneMetadata,
    TaskPriority, TaskStatus,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
pub(in crate::service_plan::store_db) struct PlanHeaderView {
    pub id: String,
    pub title: String,
    pub status: PlanStatus,
    pub project_path: String,
    pub canonical_project_path: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub owner: OrchestratorRef,
    pub planner: Option<PlannerRef>,
    pub mode: PlanMode,
    pub planning_state: crate::plan::planning_state::PlanningState,
    pub team_id: Option<String>,
    pub canvas: PlanCanvas,
}

#[derive(Deserialize, Serialize)]
pub(in crate::service_plan::store_db) struct PlanDetailsView {
    pub goal: Option<GoalView>,
    pub spec_links: Vec<SpecLink>,
    pub primary_spec_id: Option<String>,
    pub spec_refs: Vec<String>,
    pub task_groups: Vec<TaskGroup>,
    pub tag_facets: Vec<TagFacet>,
    pub milestone_kinds: Vec<MilestoneKind>,
}

#[derive(Deserialize, Serialize)]
pub(in crate::service_plan::store_db) struct GoalView {
    pub vector: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub constraints: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub postulates: Option<Vec<String>>,
}

#[derive(Deserialize, Serialize)]
pub(in crate::service_plan::store_db) struct SectionView {
    pub id: String,
    pub title: String,
    pub subsections: Vec<SectionView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canvas: Option<ElementCanvas>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<AttachmentRef>>,
}

#[derive(Deserialize, Serialize)]
pub(in crate::service_plan::store_db) struct TaskView {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub key: Option<String>,
    pub handle: Option<String>,
    pub status: TaskStatus,
    pub status_label: Option<String>,
    pub review_stage: ReviewStage,
    pub qa_state: Option<QaState>,
    pub assigned_tab_id: Option<String>,
    pub lane: Option<TaskLaneMetadata>,
    pub parent_task_id: Option<String>,
    pub depends_on: Vec<String>,
    pub section_ids: Vec<String>,
    pub group: Option<String>,
    pub model: Option<String>,
    pub peer: Option<String>,
    pub priority: TaskPriority,
    pub created_at: u64,
    pub updated_at: u64,
    pub status_changed_at: u64,
    pub tags: Vec<String>,
    pub canvas: ElementCanvas,
    pub execution_attempts: Vec<AttemptView>,
    pub criteria_count: usize,
    pub milestone_count: usize,
}

#[derive(Deserialize, Serialize)]
pub(in crate::service_plan::store_db) struct AttemptView {
    pub id: String,
    pub executor_tab_id: Option<String>,
    pub tab_id: Option<String>,
    pub executor_title: Option<String>,
    pub tab_title: Option<String>,
    pub role: Option<String>,
    pub model: Option<String>,
    pub assigned_at: Option<u64>,
    pub closed_at: Option<u64>,
}

#[derive(Serialize)]
pub(in crate::service_plan::store_db) struct ListWindowView {
    pub next_cursor: Option<ListCursorView>,
    pub total: usize,
    pub cancelled: usize,
}

#[derive(Serialize)]
pub(in crate::service_plan::store_db) struct ListCursorView {
    pub position: i64,
    pub task_id: String,
}

#[derive(Serialize)]
pub(in crate::service_plan::store_db) struct PlanView {
    #[serde(flatten)]
    pub header: PlanHeaderView,
    #[serde(flatten)]
    pub details: Option<PlanDetailsView>,
    pub sections: Vec<SectionView>,
    pub tasks: Vec<TaskView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_window: Option<ListWindowView>,
    pub task_counts: crate::service_plan::state::TaskCounts,
    pub revision: String,
}
