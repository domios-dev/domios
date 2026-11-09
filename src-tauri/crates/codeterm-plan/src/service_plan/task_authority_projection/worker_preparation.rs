use crate::service_plan::task_queries::TaskSection;
use crate::service_plan::{PlanError, PlanStore};

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct WorkerTaskPreparationRead {
    pub task_id: String,
    pub personality: Option<String>,
    pub speed: Option<String>,
    pub lane_base: Option<String>,
    pub brief: WorkerTaskBriefRead,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct WorkerTaskBriefRead {
    pub name: String,
    pub parent_task_id: Option<String>,
    pub body: String,
    pub intent: Option<String>,
    pub acceptance_criteria: Vec<String>,
    pub correctness: Option<String>,
    pub test_plan: Option<String>,
    pub role_guidance: Option<String>,
    pub executor_role: Option<String>,
    #[serde(skip_deserializing, default)]
    pub sections: Vec<TaskSection>,
    #[serde(skip_deserializing, default)]
    pub strides: Vec<TaskBriefStrideRead>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TaskBriefStrideRead {
    pub kind: String,
    pub text: String,
}

impl PlanStore {
    pub fn worker_task_preparation_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<WorkerTaskPreparationRead, PlanError> {
        self.database.worker_task_preparation_projection(plan, task)
    }
}
