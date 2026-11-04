use super::{PlanError, PlanStore};
use crate::service_plan::milestone_gates::RegressionInput;

impl PlanStore {
    pub fn retrospective_presence_projection(&self, id: &str) -> Result<Option<bool>, PlanError> {
        self.database.retrospective_presence_projection(id)
    }

    pub fn regression_gate_projection(
        &self,
        id: &str,
    ) -> Result<Option<RegressionInput>, PlanError> {
        self.database.regression_gate_input(id)
    }
}
