use super::{PlanError, PlanStore};
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanViewProjection {
    #[default]
    Summary,
    Board,
    Document,
    List,
    Chat,
    TaskMetadata,
}

#[derive(Debug, Default, Deserialize)]
pub struct ListWindowQuery {
    pub task_ref: Option<String>,
    pub task_ids: Option<String>,
    pub after_position: Option<i64>,
    pub after_task_id: Option<String>,
    pub page_size: Option<usize>,
    pub search: Option<String>,
    pub status: Option<String>,
}

impl PlanStore {
    pub fn task_reference(
        &self,
        reference: &str,
        project: Option<&str>,
        team: Option<&str>,
    ) -> Result<Value, PlanError> {
        let project = project.map(|path| {
            super::paths::project_root(std::path::Path::new(path))
                .display()
                .to_string()
        });
        self.database
            .task_reference(reference, project.as_deref(), team)
    }

    pub fn ui_plan_metadata_window(
        &self,
        id: &str,
        query: &ListWindowQuery,
    ) -> Result<Value, PlanError> {
        self.database.ui_plan_metadata_window(id, query)
    }
    pub fn ui_plan_list_window(
        &self,
        id: &str,
        query: &ListWindowQuery,
    ) -> Result<Value, PlanError> {
        self.database.ui_plan_list_window(id, query)
    }

    pub fn reference_projection(
        &self,
        plan_id: &str,
        task_ref: Option<&str>,
        section_ref: Option<&str>,
    ) -> Result<crate::plan::Plan, PlanError> {
        self.database
            .reference_projection(plan_id, task_ref, section_ref)
    }

    pub fn ui_plan_projection(
        &self,
        plan_id: &str,
        view: PlanViewProjection,
    ) -> Result<Value, PlanError> {
        self.database.ui_plan_projection(plan_id, view)
    }

    pub fn ui_plan_summaries(&self, project: Option<&str>) -> Result<Vec<Value>, PlanError> {
        self.database.ui_plan_summaries(project)
    }
}
