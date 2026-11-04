use crate::plan::{ExecutionIntent, TaskExecutionIntent, TaskLaneMetadata, TaskStatus};
use crate::service_plan::{PlanError, PlanStore};

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct TaskLaunchRead {
    pub task_id: String,
    pub name: String,
    pub status: TaskStatus,
    pub lane: TaskLaneMetadata,
    pub execution_intent: TaskExecutionIntent,
    pub dispatch_intent: ExecutionIntent,
    pub source_attempt_exists: bool,
}

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct TaskDispatchSeedRead {
    pub acceptance_criteria: Vec<String>,
    pub predecessor_tabs: Vec<String>,
}

impl PlanStore {
    pub fn task_launch_projection(
        &self,
        plan: &str,
        task: &str,
        source_attempt: Option<&str>,
    ) -> Result<TaskLaunchRead, PlanError> {
        self.database
            .task_launch_projection(plan, task, source_attempt)
    }

    pub fn task_dispatch_seed_projection(
        &self,
        plan: &str,
        task: &str,
        new_tab: &str,
    ) -> Result<TaskDispatchSeedRead, PlanError> {
        self.database
            .task_dispatch_seed_projection(plan, task, new_tab)
    }
}
