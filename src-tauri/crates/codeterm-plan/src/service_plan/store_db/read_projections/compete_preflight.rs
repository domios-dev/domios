use crate::plan::TaskStatus;
use crate::service_plan::{PlanError, PlanStore};

impl PlanStore {
    pub fn compete_review_capacity_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<(), PlanError> {
        if self.database.task_status_projection(plan, task)? == TaskStatus::Pending {
            self.dispatch_review_capacity_warning_projection(plan, task)?;
        }
        Ok(())
    }

    pub fn dispatch_review_capacity_warning_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<(), PlanError> {
        if let Some(reason) = self.database.review_gate_projection(plan)?.reason {
            log::warn!("[review-backlog] task={}: {}", task, reason);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
