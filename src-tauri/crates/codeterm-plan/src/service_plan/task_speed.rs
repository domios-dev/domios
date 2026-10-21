use super::{PlanError, PlanStore};

impl PlanStore {
    pub fn update_task_speed(
        &self,
        plan_id: &str,
        task_id: &str,
        speed: Option<&str>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |plan| {
            let task = crate::plan::task_key::find_task_mut(&mut plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            task.speed = speed.map(str::to_owned);
            task.updated_at = now_ms;
            Ok(())
        })
    }
}
