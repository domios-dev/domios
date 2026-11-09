use super::super::{PlanError, PlanStore};

impl PlanStore {
    pub fn record_attempt_checkout(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        attempt_id: &str,
        checkout_path: String,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_task_rows(
            plan_id,
            task_id,
            super::super::store_db::hot_write::TaskHistory::Attempt(attempt_id.into()),
            now_ms,
            |edit| {
                let task = &mut edit.task;
                let attempt = task
                    .execution_attempts
                    .iter_mut()
                    .find(|attempt| {
                        attempt.id == attempt_id
                            && attempt.executor_tab_id == tab_id
                            && attempt.closed_at.is_none()
                            && attempt.retired_at.is_none()
                    })
                    .ok_or_else(|| {
                        Self::task_readback_error("attempt_checkout", "live attempt missing")
                    })?;
                if attempt
                    .checkout_path
                    .as_ref()
                    .is_some_and(|bound| bound != &checkout_path)
                {
                    return Err(Self::task_readback_error(
                        "attempt_checkout",
                        "checkout binding changed",
                    ));
                }
                attempt.checkout_path = Some(checkout_path);
                task.updated_at = now_ms;
                Ok(())
            },
        )
    }
}
