use serde_json::Value;
use std::time::Instant;
use tauri::AppHandle;

use crate::plan::{Task, TaskLaneMetadata};
use crate::service::{
    verify_worker_lane, LaneProtection, ProvisionedWorkerLane, ServiceState, WorkerLaneProvenance,
};

use super::{
    arm_worker_ack_deadline, atomic_lane_commitment, await_agent_ready, cleanup_succeeded,
    close_tab_if_present, dispatch_failed, dispatch_ready_event, dispatch_safety_nudges,
    dispatch_startup_failed_event, dispatch_wait_failed, emit_plan_changed, epoch_ms,
    lane_metadata_for_dispatch, publish_action_needed_event, publish_startup_report,
    roll_back_dispatch_attempt, startup_recovery_event, startup_wait_is_recoverable,
    wait_for_tab_session, DispatchEvidence, DispatchFailureDisposition, DispatchWaitFailure,
    JsonRpcError, DISPATCH_READY_TIMEOUT,
};
pub(crate) struct DispatchStartup {
    pub(crate) app: AppHandle,
    pub(crate) svc: ServiceState,
    pub(crate) plan_id: String,
    pub(crate) task_id: String,
    pub(crate) tab_id: String,
    pub(crate) startup_attempt_id: String,
    pub(crate) attempt_id: String,
    pub(crate) parent_tab_id: String,
    pub(crate) worker_title: String,
    pub(crate) original_task: Task,
    pub(crate) original_plan_mode: crate::plan::PlanMode,
    pub(crate) lane_for_commit: TaskLaneMetadata,
    pub(crate) canonical_project_path: String,
    pub(crate) provenance: Option<WorkerLaneProvenance>,
    pub(crate) provisioned_lane: Option<ProvisionedWorkerLane>,
    pub(crate) lane_protection: LaneProtection,
    pub(crate) spawn_command: String,
}

impl DispatchStartup {
    pub(crate) fn run(self, ready_rx: tokio::sync::oneshot::Receiver<Result<(), String>>) {
        let deadline = Instant::now() + DISPATCH_READY_TIMEOUT;
        let provisioned_lane = self.provisioned_lane;
        let outcome = complete_dispatch_transaction(
            deadline,
            |deadline| await_agent_ready(ready_rx, deadline),
            |deadline| wait_for_tab_session(&self.svc, &self.tab_id, deadline),
            || match self.provenance.as_ref() {
                Some(provenance) => verify_worker_lane(
                    &provenance.canonical_cwd,
                    &provenance.worktree_path,
                    Some(&provenance.branch),
                    Some(&provenance.base),
                    self.lane_protection,
                )
                .map(Some),
                None => Ok(None),
            },
            |lane, session_id| {
                let session_binding_nonce = self
                    .svc
                    .mux
                    .read()
                    .get_tab(&self.tab_id)
                    .and_then(|tab| tab.session_binding_nonce.clone())
                    .filter(|nonce| !nonce.trim().is_empty())
                    .ok_or_else(|| "ready Worker tab has no session binding nonce".to_string())?;
                let lane_metadata = atomic_lane_commitment(
                    &lane
                        .as_ref()
                        .map(|lane| {
                            lane_metadata_for_dispatch(
                                &self.lane_for_commit,
                                lane,
                                &self.canonical_project_path,
                            )
                        })
                        .unwrap_or_else(|| {
                            let mut metadata = self.lane_for_commit.clone();
                            metadata.worktree_path = None;
                            metadata.branch = None;
                            metadata.base = None;
                            metadata.cleanup_status = None;
                            metadata
                        }),
                );
                self.svc
                    .plan_store
                    .finish_task_dispatch(
                        &self.plan_id,
                        &self.task_id,
                        &self.tab_id,
                        &self.attempt_id,
                        session_id,
                        &session_binding_nonce,
                        lane_metadata,
                        epoch_ms(),
                    )
                    .map_err(|error| error.to_string())?;
                self.svc.persist_current_attempt_session_model(&self.tab_id);
                Ok(())
            },
            || {
                roll_back_dispatch_attempt(
                    &self.svc.plan_store,
                    &self.plan_id,
                    &self.original_task,
                    &self.original_plan_mode,
                    &self.tab_id,
                    &self.attempt_id,
                    provisioned_lane,
                    || close_tab_if_present(&self.svc, &self.tab_id),
                    epoch_ms(),
                )
            },
        );

        match outcome {
            Ok(evidence) => {
                arm_worker_ack_deadline(
                    &self.svc,
                    &self.parent_tab_id,
                    &self.tab_id,
                    &self.worker_title,
                    &self.plan_id,
                    &self.task_id,
                    &self.attempt_id,
                    self.spawn_command.clone(),
                );
                emit_plan_changed(&self.app, &self.plan_id);
                let nudges = self
                    .svc
                    .plan_store
                    .show_task(&self.task_id)
                    .map(|detail| {
                        dispatch_safety_nudges(&self.plan_id, &self.task_id, &detail.task.lane)
                    })
                    .unwrap_or_default();
                publish_startup_report(
                    &self.svc,
                    &self.parent_tab_id,
                    &self.tab_id,
                    &self.worker_title,
                    &self.plan_id,
                    &self.task_id,
                    dispatch_ready_event(
                        &self.plan_id,
                        &self.task_id,
                        &self.tab_id,
                        &self.attempt_id,
                        &evidence,
                        &self.canonical_project_path,
                        &nudges,
                    ),
                );
            }
            Err(error) if startup_wait_is_recoverable(&error) => {
                // Missing readiness evidence does not prove the child is dead.
                // Keep it available for exact-tab parent recovery.
                let stage = error
                    .data
                    .as_ref()
                    .and_then(|data| data.get("stage"))
                    .and_then(Value::as_str)
                    .unwrap_or("startup")
                    .to_string();
                arm_worker_ack_deadline(
                    &self.svc,
                    &self.parent_tab_id,
                    &self.tab_id,
                    &self.worker_title,
                    &self.plan_id,
                    &self.task_id,
                    &self.attempt_id,
                    self.spawn_command.clone(),
                );
                publish_action_needed_event(
                    &self.svc,
                    &self.parent_tab_id,
                    &self.tab_id,
                    &self.worker_title,
                    startup_recovery_event(
                        &self.plan_id,
                        &self.task_id,
                        &self.tab_id,
                        &self.attempt_id,
                        Some(&self.startup_attempt_id),
                        &stage,
                    ),
                );
                emit_plan_changed(&self.app, &self.plan_id);
            }
            Err(error) => {
                let stage = error
                    .data
                    .as_ref()
                    .and_then(|data| data.get("stage"))
                    .and_then(Value::as_str)
                    .unwrap_or("startup")
                    .to_string();
                let worker_may_be_alive = error
                    .data
                    .as_ref()
                    .and_then(|data| data.get("worker_may_be_alive"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let reason = error.message.clone();
                log::warn!(
                    "[task-dispatch] dispatch_startup_failed plan={} task={} tab={} attempt={} stage={} cause={}",
                    self.plan_id,
                    self.task_id,
                    self.tab_id,
                    self.attempt_id,
                    stage,
                    reason
                );
                let attempt_finalized = if worker_may_be_alive {
                    match self.svc.plan_store.mark_worker_startup_deadline(
                        &self.plan_id,
                        &self.task_id,
                        &self.tab_id,
                        &self.attempt_id,
                        true,
                        epoch_ms(),
                    ) {
                        Ok(finalized) => finalized,
                        Err(error) => {
                            log::warn!(
                                "[task-dispatch] survivor finalization failed attempt={}: {error}",
                                self.attempt_id
                            );
                            false
                        }
                    }
                } else {
                    false
                };
                emit_plan_changed(&self.app, &self.plan_id);
                publish_startup_report(
                    &self.svc,
                    &self.parent_tab_id,
                    &self.tab_id,
                    &self.worker_title,
                    &self.plan_id,
                    &self.task_id,
                    dispatch_startup_failed_event(
                        &self.plan_id,
                        &self.task_id,
                        &self.tab_id,
                        &self.attempt_id,
                        &stage,
                        &reason,
                        if worker_may_be_alive {
                            DispatchFailureDisposition::Survivor { attempt_finalized }
                        } else {
                            DispatchFailureDisposition::Cleanup {
                                ok: cleanup_succeeded(&stage, &reason),
                            }
                        },
                    ),
                );
            }
        }
    }
}

/// Keep uncertain startups available for parent recovery; roll back only when
/// failure evidence is definitive.
pub(crate) fn complete_dispatch_transaction<Ready, Binding, Provenance, Commit, Rollback>(
    deadline: Instant,
    await_ready: Ready,
    wait_binding: Binding,
    validate_provenance: Provenance,
    commit: Commit,
    rollback: Rollback,
) -> Result<DispatchEvidence, JsonRpcError>
where
    Ready: FnOnce(Instant) -> Result<(), DispatchWaitFailure>,
    Binding: FnOnce(Instant) -> Result<String, DispatchWaitFailure>,
    Provenance: FnOnce() -> Result<Option<WorkerLaneProvenance>, String>,
    Commit: FnOnce(&Option<WorkerLaneProvenance>, &str) -> Result<(), String>,
    Rollback: FnOnce() -> Result<(), String>,
{
    let mut worker_may_be_alive = false;
    let outcome = (|| {
        await_ready(deadline).map_err(|failure| {
            worker_may_be_alive = !failure.is_definitive();
            dispatch_wait_failed("agent_ready", failure)
        })?;
        let session_id = wait_binding(deadline).map_err(|failure| {
            worker_may_be_alive = !failure.is_definitive();
            dispatch_wait_failed("session_binding", failure)
        })?;
        let lane =
            validate_provenance().map_err(|error| dispatch_failed("provenance_readback", error))?;
        commit(&lane, &session_id).map_err(|error| dispatch_failed("task_commit", error))?;
        Ok(DispatchEvidence { session_id, lane })
    })();
    match outcome {
        Ok(evidence) => Ok(evidence),
        // Readiness evidence being absent is not proof the dispatch failed:
        // never destroy a lane whose worker may still be alive.
        Err(error) if worker_may_be_alive => Err(error),
        Err(error) => match rollback() {
            Ok(()) => Err(error),
            Err(cleanup) => Err(dispatch_failed("compensation", cleanup)),
        },
    }
}
