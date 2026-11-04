use crate::service_plan::{PlanError, PlanStore};

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BatchTaskRefusalRead {
    pub task_id: String,
    pub name: String,
    pub assigned_tab_id: Option<String>,
}

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskSelectorRead {
    pub index: usize,
    pub task_id: String,
}

impl PlanStore {
    pub fn batch_task_refusal_projection(
        &self,
        plan: &str,
        selectors: &[String],
        caller_tab: Option<&str>,
    ) -> Result<Option<BatchTaskRefusalRead>, PlanError> {
        self.database
            .batch_task_refusal_projection(plan, selectors, caller_tab)
    }

    pub fn batch_task_resolution_projection(
        &self,
        plan: &str,
        selectors: &[String],
    ) -> Result<Vec<TaskSelectorRead>, PlanError> {
        self.database
            .batch_task_resolution_projection(plan, selectors)
    }
}
