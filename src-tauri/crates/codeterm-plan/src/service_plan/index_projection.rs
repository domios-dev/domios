use super::{IndexEntry, PlanError, PlanStore};
use crate::plan::PlanStatus;

impl PlanStore {
    pub fn plan_list_index(
        &self,
        project: Option<&str>,
        status: Option<PlanStatus>,
        unowned: bool,
        owner: Option<&str>,
    ) -> Result<Vec<IndexEntry>, PlanError> {
        self.database
            .plan_list_index(project, status, unowned, owner)
    }
}
