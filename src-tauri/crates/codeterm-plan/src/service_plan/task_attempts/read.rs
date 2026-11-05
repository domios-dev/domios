use crate::plan::{Milestone, TaskExecutionAttempt, TaskLaneMetadata};

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub(in crate::service_plan) struct TaskAttemptRead {
    pub id: String,
    pub prompt: Option<TaskAttemptPromptRead>,
    #[serde(default)]
    pub execution_attempts: Vec<TaskExecutionAttempt>,
    #[serde(default)]
    pub milestones: Vec<Milestone>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub(in crate::service_plan) struct TaskAttemptPromptRead {
    pub id: String,
    pub name: String,
    pub body: String,
    pub model: Option<String>,
    pub personality: Option<String>,
    pub role_guidance: Option<String>,
    pub executor_role: Option<String>,
    pub intent: Option<String>,
    pub correctness: Option<String>,
    pub test_plan: Option<String>,
    pub acceptance_criteria: Vec<String>,
    pub lane: TaskLaneMetadata,
}
