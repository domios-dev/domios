use crate::service_plan::{PlanError, PlanStore};

pub struct RuntimeAttemptRestoreQuery<'a> {
    pub task_ref: &'a str,
    pub executor_tab_id: &'a str,
    pub provider: Option<&'a str>,
    pub session_id: Option<&'a str>,
    pub nonce: Option<&'a str>,
    pub reported_nonce: Option<&'a str>,
}

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct RuntimeAttemptRestoreRead {
    pub plan_id: String,
    pub candidates: Vec<String>,
    pub restart_acknowledged_at: Option<u64>,
}

impl PlanStore {
    pub fn runtime_attempt_restore_projection(
        &self,
        query: &RuntimeAttemptRestoreQuery<'_>,
    ) -> Result<RuntimeAttemptRestoreRead, PlanError> {
        self.database.runtime_attempt_restore_projection(query)
    }
}
