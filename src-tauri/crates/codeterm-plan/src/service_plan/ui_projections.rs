use super::{PlanError, PlanStore};
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, ts_rs::TS)]
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

#[derive(Debug, Deserialize)]
pub struct ReferenceProjection {
    pub title: String,
    pub status: crate::plan::PlanStatus,
    pub owner: crate::plan::OrchestratorRef,
    pub canonical_project_path: String,
    pub tasks: Vec<ReferenceTaskRead>,
    pub task_refs: std::collections::BTreeMap<String, Option<String>>,
    pub sections: Vec<ReferenceSectionRead>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct ReferenceTaskRead {
    pub id: String,
    pub name: String,
    pub key: Option<String>,
    pub handle: Option<String>,
}

impl ReferenceTaskRead {
    pub fn display_key(&self) -> String {
        self.key
            .clone()
            .unwrap_or_else(|| crate::plan::task_key::fallback_key(&self.id))
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct ReferenceSectionRead {
    pub id: String,
    pub title: String,
    pub handle: Option<String>,
}

impl ReferenceProjection {
    pub fn task(&self, reference: &str) -> Option<&ReferenceTaskRead> {
        let id = self.task_refs.get(reference)?.as_ref()?;
        self.tasks.iter().find(|task| &task.id == id)
    }
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
        task_refs: &[String],
        section_refs: &[String],
    ) -> Result<ReferenceProjection, PlanError> {
        self.database
            .reference_projection(plan_id, task_refs, section_refs)
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
