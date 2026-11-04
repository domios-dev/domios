use crate::service_plan::{tag_model, PlanError, PlanStore};

impl PlanStore {
    pub fn tag_report(&self, id: &str) -> Result<tag_model::TagReport, PlanError> {
        self.database
            .tag_report_input(id)
            .map(tag_model::tag_report)
    }
}
