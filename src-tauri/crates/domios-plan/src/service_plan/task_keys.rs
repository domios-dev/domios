use super::ports::{TaskKeyPort, TaskKeyScope};
use super::PlanStore;
use crate::plan::task_key;
use std::collections::HashMap;
use std::sync::Arc;

impl PlanStore {
    pub fn bind_task_key_port(&self, port: Arc<dyn TaskKeyPort>) {
        *self.task_key_port.write() = Some(port);
    }

    /// Largest stored key number per prefix across every loaded plan.
    pub fn task_key_high_water(&self) -> HashMap<String, u64> {
        let plans: Vec<_> = self.plans.read().values().cloned().collect();
        let mut keys = Vec::new();
        for plan in plans {
            let plan = plan.lock();
            keys.extend(plan.tasks.iter().filter_map(|task| task.key.clone()));
        }
        task_key::high_water(keys.iter().map(String::as_str))
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
        let plan = self.plans.read().get(plan_id).cloned()?;
        let plan = plan.lock();
        Some(TaskKeyScope {
            plan_id: plan.id.clone(),
            team_id: plan.team_id.clone(),
            project_path: plan.project_path.clone(),
            canonical_project_path: plan.canonical_project_path.clone(),
        })
    }

    pub fn task_display_key(&self, plan_id: &str, task_ref: &str) -> Option<String> {
        let plan = self.plans.read().get(plan_id).cloned()?;
        let plan = plan.lock();
        task_key::find_task(&plan.tasks, task_ref).map(|task| task.display_key())
    }
}
