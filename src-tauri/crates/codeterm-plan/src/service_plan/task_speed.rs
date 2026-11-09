use super::{PlanError, PlanStore};

impl PlanStore {
    pub fn update_task_speed(
        &self,
        plan_id: &str,
        task_id: &str,
        speed: Option<&str>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_task_rows(
            plan_id,
            task_id,
            super::store_db::hot_write::TaskHistory::Core,
            now_ms,
            |edit| {
                let task = &mut edit.task;
                task.speed = speed.map(str::to_owned);
                task.updated_at = now_ms;
                Ok(())
            },
        )
    }
}
