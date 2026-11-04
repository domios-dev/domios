use crate::plan::TaskStatus;
use crate::service_plan::{PlanError, PlanStore};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub struct TaskContinuityDigest(String);

impl TaskContinuityDigest {
    pub(in crate::service_plan) fn from_bytes(bytes: Vec<u8>) -> Result<Self, PlanError> {
        if bytes.len() != 32 {
            return Err(PlanError::Io(
                "task continuity digest is not SHA-256".into(),
            ));
        }
        Ok(Self(
            bytes.iter().map(|byte| format!("{byte:02x}")).collect(),
        ))
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TaskContinuityRead {
    pub plan_id: String,
    pub task_id: String,
    pub status: TaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assigned_tab_id: Option<String>,
    pub owner_epoch: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_tab_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_session_id: Option<String>,
    pub digest: TaskContinuityDigest,
}

impl PlanStore {
    pub fn task_continuity_projection(
        &self,
        task_ref: &str,
    ) -> Result<TaskContinuityRead, PlanError> {
        self.database.task_continuity_projection(task_ref)
    }
}
