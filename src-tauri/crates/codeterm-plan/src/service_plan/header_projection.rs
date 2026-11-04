use super::{PlanError, PlanStore};
use crate::plan::OrchestratorRef;

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct PlanIdentityRead {
    pub owner: OrchestratorRef,
    pub team_id: Option<String>,
}

impl PlanStore {
    pub fn plan_identity_projection(&self, id: &str) -> Result<PlanIdentityRead, PlanError> {
        self.database.plan_identity_projection(id)
    }
}
