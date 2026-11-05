use super::{PlanError, PlanStore};
use crate::plan::{OrchestratorRef, PlanMode};

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct PlanIdentityRead {
    pub owner: OrchestratorRef,
    pub team_id: Option<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct NoticeDeliveryTargetRead {
    pub owner_tab_id: Option<String>,
    pub mode: PlanMode,
}

impl PlanStore {
    pub fn notice_delivery_target_projection(
        &self,
        id: &str,
    ) -> Result<NoticeDeliveryTargetRead, PlanError> {
        self.database.notice_delivery_target_projection(id)
    }

    pub fn plan_identity_projection(&self, id: &str) -> Result<PlanIdentityRead, PlanError> {
        self.database.plan_identity_projection(id)
    }
}
