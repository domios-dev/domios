use crate::plan::{TaskExecutionIntent, TaskStatus};
use crate::service_plan::{PlanError, PlanStore};

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TaskDispatchPreflightRead {
    pub plan_id: String,
    pub project_path: String,
    pub canonical_project_path: String,
    pub owner_tab_id: Option<String>,
    pub model: Option<String>,
    pub lane: crate::plan::TaskLaneMetadata,
    pub files: crate::plan::TaskFiles,
    pub kind: String,
    pub build_breaking: bool,
    pub id: String,
    pub name: String,
    pub status: TaskStatus,
    pub pause_reason: Option<String>,
    pub status_label: Option<String>,
    pub execution_intent: TaskExecutionIntent,
    pub is_unplanned_work: bool,
}

impl PlanStore {
    pub fn task_dispatch_preflight_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<TaskDispatchPreflightRead, PlanError> {
        self.database.task_dispatch_preflight_projection(plan, task)
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct DispatchCheckoutOccupantRead {
    pub task_id: String,
    pub attempt_id: String,
    pub executor_tab_id: Option<String>,
}

impl PlanStore {
    pub fn dispatch_checkout_occupant_projection(
        &self,
        plan: &str,
        task: &str,
        checkout: &str,
        live_tabs: &[String],
    ) -> Result<Option<DispatchCheckoutOccupantRead>, PlanError> {
        self.database
            .dispatch_checkout_occupant_projection(plan, task, checkout, live_tabs)
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct RemoteLaneIntegrationRead {
    pub canonical_path: String,
    pub task_destination: Option<String>,
    pub destination: Option<String>,
    pub judge_verdict: Option<String>,
    pub report_sha: Option<String>,
    pub remote_dispatch_state: Option<crate::plan::RemoteDispatchState>,
    pub remote_lane_tip: Option<String>,
    pub remote_merge_commit: Option<String>,
    pub mesh_peer: Option<String>,
    pub branch: Option<String>,
    pub base: Option<String>,
}

impl PlanStore {
    pub fn remote_lane_integration_projection(
        &self,
        plan: &str,
        task: &str,
        attempt: &str,
    ) -> Result<RemoteLaneIntegrationRead, PlanError> {
        self.database
            .remote_lane_integration_projection(plan, task, attempt)
    }
}
