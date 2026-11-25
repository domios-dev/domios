use crate::plan::RemoteDispatchState;
use crate::service_plan::{PlanError, PlanStore};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RemoteWorkerBindingRead {
    pub id: String,
    pub executor_tab_id: String,
    pub mesh_peer: Option<String>,
    pub session_binding_nonce: Option<String>,
    pub closed_at: Option<u64>,
    pub retired_at: Option<u64>,
}

impl RemoteWorkerBindingRead {
    pub fn can_observe(&self, peer: &str, tab: &str) -> bool {
        self.closed_at.is_none()
            && self.retired_at.is_none()
            && self.mesh_peer.as_deref() == Some(peer)
            && self.executor_tab_id == tab
    }
}

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RemoteRecoveryAttemptRead {
    #[serde(flatten)]
    pub binding: RemoteWorkerBindingRead,
    pub acknowledged_at: Option<u64>,
    pub assigned_at: Option<u64>,
    pub remote_dispatch_state: Option<RemoteDispatchState>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RemoteAttemptDeliveryRead {
    pub executor_tab_id: String,
    pub executor_title: Option<String>,
    pub orchestrator_tab_id: Option<String>,
    pub parent_tab_id: Option<String>,
    pub owner_tab_id: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub requested_model: Option<String>,
    pub provider_resolved_model: Option<String>,
    pub session_model: Option<String>,
    pub branch: Option<String>,
    pub base: Option<String>,
    pub mesh_peer: Option<String>,
}

impl RemoteAttemptDeliveryRead {
    pub fn target_orchestrator_tab_id(&self) -> Option<&str> {
        [
            self.owner_tab_id.as_deref(),
            self.orchestrator_tab_id.as_deref(),
            self.parent_tab_id.as_deref(),
        ]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|tab_id| !tab_id.is_empty())
    }
}

impl PlanStore {
    pub fn remote_worker_binding_projection(
        &self,
        plan: &str,
        task: &str,
        attempt: &str,
    ) -> Result<Option<RemoteWorkerBindingRead>, PlanError> {
        self.database
            .remote_worker_binding_projection(plan, task, attempt)
    }

    pub fn remote_attempt_delivery_projection(
        &self,
        plan: &str,
        task: &str,
        attempt: &str,
    ) -> Result<Option<RemoteAttemptDeliveryRead>, PlanError> {
        self.database
            .remote_attempt_delivery_projection(plan, task, attempt)
    }

    pub fn remote_recovery_attempts_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<Vec<RemoteRecoveryAttemptRead>, PlanError> {
        self.database
            .remote_recovery_attempts_projection(plan, task)
    }
}
