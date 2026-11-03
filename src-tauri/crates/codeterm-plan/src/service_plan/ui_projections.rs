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
}

impl PlanStore {
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
