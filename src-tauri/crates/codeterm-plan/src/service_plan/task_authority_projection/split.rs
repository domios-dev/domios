use crate::plan::TaskStatus;
use crate::service_plan::{PlanError, PlanStore};

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct TaskSplitDefaultsRead {
    pub parent_task_id: Option<String>,
    pub status: TaskStatus,
    pub kind: String,
    pub body: Option<String>,
    pub acceptance_criteria: Vec<String>,
    pub section_ids: Vec<String>,
    pub test_plan: Option<String>,
    pub qa_plan: Option<String>,
    pub correctness: Option<String>,
    pub executor_role: Option<String>,
    pub personality: Option<String>,
    pub role_guidance: Option<String>,
    pub model: Option<String>,
    pub model_rationale: Option<String>,
    pub intent: Option<String>,
}

impl PlanStore {
    pub fn task_split_defaults_projection(
        &self,
        task_ref: &str,
        include_body: bool,
    ) -> Result<TaskSplitDefaultsRead, PlanError> {
        self.database
            .task_split_defaults_projection(task_ref, include_body)
    }
}
