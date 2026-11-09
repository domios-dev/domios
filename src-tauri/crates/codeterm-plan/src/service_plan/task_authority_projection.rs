use super::{PlanError, PlanStore};
use crate::plan::{PlanMode, PlanningState, TaskFiles};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TaskOwnerEpochRead {
    pub plan_id: String,
    pub task_id: String,
    pub owner_epoch: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_tab_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_session_id: Option<String>,
    pub handoff_required: bool,
}

impl TaskOwnerEpochRead {
    pub fn matches_owner_epoch(&self, tab_id: &str, owner_epoch: u64) -> bool {
        !self.handoff_required
            && self.owner_epoch == owner_epoch
            && self.owner_tab_id.as_deref() == Some(tab_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TaskContextFilesRead {
    pub plan_id: String,
    pub task_id: String,
    pub files: TaskFiles,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PlanTaskAdmissionRead {
    pub mode: PlanMode,
    pub planning_state: PlanningState,
}

impl PlanTaskAdmissionRead {
    pub fn execution_tasks_unlocked(&self) -> bool {
        !matches!(self.mode, PlanMode::Complete | PlanMode::Paused)
            && (self.planning_state.allows_execution_tasks()
                || matches!(self.mode, PlanMode::Approved | PlanMode::Execution))
    }
}

pub(super) struct TabAttemptOwnerRead {
    pub plan_id: String,
    pub task_id: String,
    pub attempt_id: Option<String>,
    pub assigned_tab: Option<String>,
    pub competition_group: Option<String>,
    pub session_id: Option<String>,
    pub nonce: Option<String>,
}

impl PlanStore {
    pub fn task_status_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<crate::plan::TaskStatus, PlanError> {
        self.database.task_status_projection(plan, task)
    }

    pub fn task_assignment_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<Option<String>, PlanError> {
        self.database.task_assignment_projection(plan, task)
    }

    pub fn plan_skill_receipt_projection(
        &self,
        plan: &str,
        team: Option<&str>,
        tab: &str,
    ) -> Result<bool, PlanError> {
        self.database.plan_skill_receipt_projection(plan, team, tab)
    }

    pub fn plan_work_facet_projection(&self, plan_id: &str) -> Result<bool, PlanError> {
        self.database.plan_work_facet_projection(plan_id)
    }

    pub fn task_name_projection(&self, task_ref: &str) -> Result<String, PlanError> {
        self.database.task_name_projection(task_ref)
    }

    pub fn task_authority_target_projection(
        &self,
        task_ref: &str,
    ) -> Result<super::TaskBindingRead, PlanError> {
        self.database.task_authority_target_projection(task_ref)
    }

    pub fn plan_task_admission_projection(
        &self,
        plan_id: &str,
    ) -> Result<PlanTaskAdmissionRead, PlanError> {
        self.database.plan_task_admission_projection(plan_id)
    }

    pub fn unplanned_task_section_projection(
        &self,
        plan_id: &str,
        parent_task_id: Option<&str>,
    ) -> Result<Option<String>, PlanError> {
        self.database
            .unplanned_task_section_projection(plan_id, parent_task_id)
    }

    pub fn task_context_files_projection(
        &self,
        task_ref: &str,
    ) -> Result<TaskContextFilesRead, PlanError> {
        self.database.task_context_files_projection(task_ref)
    }

    pub fn task_owner_epoch_projection(
        &self,
        task_ref: &str,
    ) -> Result<TaskOwnerEpochRead, PlanError> {
        self.database.task_owner_epoch_projection(task_ref)
    }
}

pub(super) mod worker_transfer;

pub(super) mod startup;

mod continuity;
pub use continuity::{TaskContinuityDigest, TaskContinuityRead};

mod checkpoint;
mod creation;
pub use checkpoint::TaskCheckpointAdoptionRead;
mod restart;
mod restoration;
mod runtime_restore;
mod runtime_session;
mod split;
pub use creation::{TaskCreationRead, TaskDescriptionRead};
pub use restart::{
    CompletedTaskRestartRead, RestartAttemptRead, TaskAttemptBindingRead, TaskRestartAuthorityRead,
    TaskRestartQuery,
};
pub use restoration::{ReviewBindingRestoreRead, ReviewRestoreAttemptRead, TaskBindingRestoreRead};
pub use runtime_restore::{RuntimeAttemptRestoreQuery, RuntimeAttemptRestoreRead};
pub use runtime_session::{RuntimeSessionAttemptRead, TaskRuntimeSessionRead};
pub use split::TaskSplitDefaultsRead;

mod provider_interruption;
pub use provider_interruption::{ProviderRecoveryRead, TaskInterruptionRead};

mod remote;
pub use remote::{RemoteRecoveryAttemptRead, RemoteWorkerBindingRead};

mod review;
pub use review::{CanonicalWriterHoldRead, LaneIntegrationRead};

mod lane_admission;
pub use lane_admission::{TaskLaneAdmissionRead, TaskLaneOwnerRead};

mod batch;
pub use batch::{BatchTaskRefusalRead, TaskSelectorRead};

mod shared_inspection;
pub(super) use shared_inspection::{SharedDispatchRead, SharedReportRead};

mod plan_metadata;

mod respawn_source;
pub(super) use respawn_source::{ensure_respawn_source_fields, TaskRespawnSourceRead};

mod worker_loss;
pub use worker_loss::{WorkerLossRead, WorkerLossRecoveryRead, WorkerLossReplacementRead};

mod ack;
pub(super) use ack::{acknowledging_binding_matches, TaskAckAuthorityRead};

mod launch;
pub use launch::{TaskDispatchSeedRead, TaskLaunchRead};

mod deadline;

mod spawn_binding;
pub use spawn_binding::{SpawnAttemptBindingRead, TaskSpawnBindingRead};

mod worker_preparation;
pub use worker_preparation::{TaskBriefStrideRead, WorkerTaskBriefRead, WorkerTaskPreparationRead};

mod dispatch_preflight;
pub use dispatch_preflight::{
    DispatchCheckoutOccupantRead, RemoteLaneIntegrationRead, TaskDispatchPreflightRead,
};

pub(super) fn current_skill_from_rows(
    rows: &[TabAttemptOwnerRead],
    tab: &str,
    session: &str,
    nonce: &str,
) -> Result<Option<(String, String, String)>, PlanError> {
    if tab.trim().is_empty() || session.trim().is_empty() || nonce.trim().is_empty() {
        return Err(PlanError::Io(
            "plan skill caller identity is incomplete".into(),
        ));
    }
    let mut current = None;
    for row in rows {
        let attempt = row
            .attempt_id
            .clone()
            .ok_or_else(|| PlanError::Io("plan skill assignment has no current attempt".into()))?;
        if current.is_some()
            || (row.competition_group.is_none() && row.assigned_tab.as_deref() != Some(tab))
            || row.session_id.as_deref() != Some(session)
            || row.nonce.as_deref() != Some(nonce)
        {
            return Err(PlanError::Io(
                "plan skill caller does not identify exactly one current attempt".into(),
            ));
        }
        current = Some((row.plan_id.clone(), row.task_id.clone(), attempt));
    }
    Ok(current)
}
