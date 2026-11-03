use super::ports::{TaskKeyPort, TaskKeyScope};
use super::PlanStore;
use crate::plan::task_key;
use std::collections::HashMap;
use std::sync::Arc;

impl PlanStore {
    pub fn bind_task_key_port(&self, port: Arc<dyn TaskKeyPort>) {
        *self.task_key_port.write() = Some(port);
    }

    /// Largest stored key number per prefix, read from the task rows; no plan is loaded.
    pub fn task_key_high_water(&self) -> HashMap<String, u64> {
        let keys = self.task_field_values("$.key");
        task_key::high_water(keys.iter().map(|row| row.value.as_str()))
    }

    /// Must NOT be called while holding a plan mutex: the host port takes the mux lock.
    pub(super) fn issue_task_keys(&self, plan_id: &str, count: usize) -> Vec<String> {
        if count == 0 {
            return Vec::new();
        }
        let Some(port) = self.task_key_port.read().clone() else {
            return Vec::new();
        };
        let Some(scope) = self.task_key_scope(plan_id) else {
            return Vec::new();
        };
        let high_water = self.task_key_high_water();
        port.issue_task_keys(&scope, count, &high_water)
    }

    fn task_key_scope(&self, plan_id: &str) -> Option<TaskKeyScope> {
        self.database.task_key_scope(plan_id).ok().flatten()
    }

    pub fn task_display_key(&self, plan_id: &str, task_ref: &str) -> Option<String> {
        let rows = self.database.scoped_task_references(plan_id).ok()?;
        let display_key = |row: &super::store_db::TaskRefRow| {
            row.key
                .clone()
                .unwrap_or_else(|| task_key::fallback_key(&row.task_id))
        };
        if let Some(row) = rows.iter().find(|row| row.task_id == task_ref) {
            return Some(display_key(row));
        }
        let mut matches = rows.iter().filter(|row| {
            task_key::name_matches(row.handle.as_deref(), &display_key(row), task_ref)
        });
        let key = display_key(matches.next()?);
        matches.next().is_none().then_some(key)
    }
}
