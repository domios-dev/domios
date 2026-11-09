use super::{PlanError, PlanStore};

pub struct CuratorNoticeRead {
    pub owner_tab_id: Option<String>,
    pub body: String,
}

impl PlanStore {
    pub fn curator_notice_projection(
        &self,
        plan_id: &str,
        now_ms: u64,
    ) -> Result<CuratorNoticeRead, PlanError> {
        self.database.curator_notice_projection(plan_id, now_ms)
    }
}
