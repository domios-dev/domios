use crate::plan::TaskStatus;
use crate::service_plan::{PlanError, PlanStore, TaskContinuityDigest, TaskContinuityRead};

pub struct TaskRestartQuery<'a> {
    pub task_ref: &'a str,
    pub attempt_id: &'a str,
    pub executor_tab_id: &'a str,
    pub provider: &'a str,
    pub session_id: Option<&'a str>,
    pub nonce: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct RestartAttemptRead {
    pub id: String,
    pub executor_tab_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_binding_nonce: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acknowledged_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retired_at: Option<u64>,
    pub owner_epoch: u64,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct TaskRestartAuthorityRead {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt: Option<RestartAttemptRead>,
    pub attempt_id_count: u64,
    pub other_open_attempts: u64,
    pub unclosed_attempts: u64,
    pub completed_source_attempts: u64,
    pub provider_attempts: u64,
    pub runtime_attempts: u64,
}

#[derive(Debug)]
pub struct CompletedTaskRestartRead {
    pub authority: TaskRestartAuthorityRead,
    pub digest: TaskContinuityDigest,
}

impl CompletedTaskRestartRead {
    pub fn continuity(&self) -> TaskContinuityRead {
        TaskContinuityRead {
            plan_id: self.authority.plan_id.clone(),
            task_id: self.authority.task_id.clone(),
            status: self.authority.status.clone(),
            assigned_tab_id: self.authority.assigned_tab_id.clone(),
            owner_epoch: self.authority.owner_epoch,
            owner_tab_id: self.authority.owner_tab_id.clone(),
            owner_session_id: self.authority.owner_session_id.clone(),
            digest: self.digest.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TaskAttemptBindingRead {
    pub plan_id: String,
    pub task_id: String,
    pub attempt_id: String,
}

impl PlanStore {
    pub fn task_restart_authority_projection(
        &self,
        query: &TaskRestartQuery<'_>,
    ) -> Result<TaskRestartAuthorityRead, PlanError> {
        self.database.task_restart_authority_projection(query)
    }

    pub fn task_completed_restart_projection(
        &self,
        query: &TaskRestartQuery<'_>,
    ) -> Result<Option<CompletedTaskRestartRead>, PlanError> {
        self.database.task_completed_restart_projection(query)
    }

    /// Returns at most two bindings, sufficient to distinguish absent, unique and ambiguous.
    pub fn runtime_unclosed_attempt_bindings_projection(
        &self,
        tab: &str,
    ) -> Result<Vec<TaskAttemptBindingRead>, PlanError> {
        self.database
            .runtime_attempt_bindings_projection(tab, false)
    }

    /// Returns at most two bindings after excluding retired latest-unclosed attempts.
    pub fn runtime_current_attempt_bindings_projection(
        &self,
        tab: &str,
    ) -> Result<Vec<TaskAttemptBindingRead>, PlanError> {
        self.database.runtime_attempt_bindings_projection(tab, true)
    }
}
