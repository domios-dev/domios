use super::ports::TaskKeyPort;
use super::{PlanError, PlanStore};
use std::collections::HashMap;
use std::sync::Arc;

impl PlanStore {
    pub fn bind_task_key_port(&self, port: Arc<dyn TaskKeyPort>) {
        *self.task_key_port.write() = Some(port);
    }

    pub fn task_key_high_water(&self) -> Result<HashMap<String, u64>, PlanError> {
        self.database.task_key_high_water()
    }

    /// Must NOT be called while holding a plan mutex: the host port takes the mux lock.
    pub(super) fn issue_task_keys(
        &self,
        plan_id: &str,
        count: usize,
    ) -> Result<Vec<String>, PlanError> {
        if count == 0 {
            return Ok(Vec::new());
        }
        let Some(port) = self.task_key_port.read().clone() else {
            return Ok(Vec::new());
        };
        let Some(input) = self.database.task_key_issue_input(plan_id)? else {
            return Ok(Vec::new());
        };
        Ok(port.issue_task_keys(&input.scope, count, &input.high_water))
    }

    pub fn task_display_key(
        &self,
        plan_id: &str,
        task_ref: &str,
    ) -> Result<Option<String>, PlanError> {
        self.database.task_display_key(plan_id, task_ref)
    }
}
