use crate::service_plan::{PlanError, PlanStore};

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct TaskCreationRead {
    pub key: String,
    pub section_ids: Vec<String>,
}

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct TaskDescriptionRead {
    pub name: String,
    pub assigned_tab_id: Option<String>,
}

impl PlanStore {
    pub fn task_description_projection(
        &self,
        task_ref: &str,
    ) -> Result<TaskDescriptionRead, PlanError> {
        self.database.task_description_projection(task_ref)
    }

    pub fn task_creation_projection(
        &self,
        plan_id: &str,
        task_ref: &str,
    ) -> Result<Option<TaskCreationRead>, PlanError> {
        self.database.task_creation_projection(plan_id, task_ref)
    }
}
