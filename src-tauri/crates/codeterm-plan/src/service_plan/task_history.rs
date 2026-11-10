use super::{PlanError, PlanStore};
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Copy, Debug, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum TaskHistoryKind {
    Milestones,
    Attempts,
}

impl PlanStore {
    pub fn task_history_page(
        &self,
        plan_id: &str,
        task_ref: &str,
        kind: TaskHistoryKind,
        before: Option<i64>,
        limit: usize,
    ) -> Result<Value, PlanError> {
        self.database
            .task_history_page(plan_id, task_ref, kind, before, limit)
    }
}
