use crate::plan::PlanStatus;
use crate::service_plan::{PlanError, PlanStore};

impl PlanStore {
    pub fn plan_exists_projection(&self, plan: &str) -> Result<bool, PlanError> {
        self.database.plan_exists_projection(plan)
    }

    pub fn plan_owner_tab_projection(&self, plan: &str) -> Result<Option<String>, PlanError> {
        self.database.plan_owner_tab_projection(plan)
    }

    pub fn plan_status_projection(&self, plan: &str) -> Result<PlanStatus, PlanError> {
        self.database.plan_status_projection(plan)
    }

    pub fn plan_project_ids_projection(&self, project: &str) -> Result<Vec<String>, PlanError> {
        self.database.plan_project_ids_projection(project)
    }
}
