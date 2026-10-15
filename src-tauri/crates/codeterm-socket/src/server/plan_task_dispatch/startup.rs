use serde_json::Value;
use std::time::Instant;

use codeterm_service_core::service::{verify_worker_lane, WorkerLaneProvenance};

use super::{
    arm_worker_ack_deadline, atomic_lane_commitment, await_agent_ready, cleanup_succeeded,
    close_tab_if_present, dispatch_failed, dispatch_ready_event, dispatch_safety_nudges,
    dispatch_startup_failed_event, dispatch_wait_failed, emit_plan_changed, epoch_ms,
    lane_metadata_for_dispatch, publish_action_needed_event, publish_startup_report,
    roll_back_dispatch_attempt, startup_recovery_event, startup_wait_is_recoverable,
    wait_for_tab_session, DispatchEvidence, DispatchFailureDisposition, DispatchWaitFailure,
    JsonRpcError, DISPATCH_READY_TIMEOUT,
};
pub(crate) use codeterm_service_core::ports::socket::DispatchStartup;

pub(crate) fn run_startup(
    this: DispatchStartup,
    ready_rx: tokio::sync::oneshot::Receiver<Result<(), String>>,
) {
    let deadline = Instant::now() + DISPATCH_READY_TIMEOUT;
    let provisioned_lane = this.provisioned_lane;
    let outcome = complete_dispatch_transaction(
        deadline,
        |deadline| {
            await_agent_ready(ready_rx, deadline).map_err(|failure| match failure {
                DispatchWaitFailure::Definitive(reason) => DispatchWaitFailure::Definitive(
                    match this.svc.launch_output_tail(&this.tab_id) {
                        Some(tail) => format!("{reason}\nprovider output (tail):\n{tail}"),
                        None => reason,
                    },
                ),
                uncertain => uncertain,
            })
        },
        |deadline| wait_for_tab_session(&this.svc, &this.tab_id, deadline),
        || match this.provenance.as_ref() {
            Some(provenance) => verify_worker_lane(
                &provenance.canonical_cwd,
                &provenance.worktree_path,
                Some(&provenance.branch),
                Some(&provenance.base),
                this.lane_protection,
            )
            .map(Some),
            None => Ok(None),
        },
        |lane, session_id| {
            let session_binding_nonce = this
                .svc
                .mux
                .read()
                .get_tab(&this.tab_id)
                .and_then(|tab| tab.session_binding_nonce.clone())
                .filter(|nonce| !nonce.trim().is_empty())
                .ok_or_else(|| "ready Worker tab has no session binding nonce".to_string())?;
            let lane_metadata = atomic_lane_commitment(
                &lane
                    .as_ref()
                    .map(|lane| {
                        lane_metadata_for_dispatch(
                            &this.lane_for_commit,
                            lane,
                            &this.canonical_project_path,
                        )
                    })
                    .unwrap_or_else(|| {
                        let mut metadata = this.lane_for_commit.clone();
                        metadata.worktree_path = None;
                        metadata.branch = None;
                        metadata.base = None;
                        metadata.cleanup_status = None;
                        metadata
                    }),
            );
            this.svc
                .plan_store
                .finish_task_dispatch(
                    &this.plan_id,
                    &this.task_id,
                    &this.tab_id,
                    &this.attempt_id,
                    session_id,
                    &session_binding_nonce,
                    lane_metadata,
                    epoch_ms(),
                )
                .map_err(|error| error.to_string())?;
            this.svc.persist_current_attempt_session_model(&this.tab_id);
            Ok(())
        },
        || {
            roll_back_dispatch_attempt(
                &this.svc.plan_store,
                &this.plan_id,
                &this.original_task,
                &this.original_plan_mode,
                &this.tab_id,
                &this.attempt_id,
                provisioned_lane,
                || close_tab_if_present(&this.svc, &this.tab_id),
                epoch_ms(),
            )
        },
    );

    match outcome {
        Ok(evidence) => {
            arm_worker_ack_deadline(
                &this.svc,
                &this.parent_tab_id,
                &this.tab_id,
                &this.worker_title,
                &this.plan_id,
                &this.task_id,
                &this.attempt_id,
                this.spawn_command.clone(),
            );
            emit_plan_changed(&this.app, &this.plan_id);
            let nudges = this
                .svc
                .plan_store
                .show_task(&this.task_id)
                .map(|detail| {
                    dispatch_safety_nudges(&this.plan_id, &this.task_id, &detail.task.lane)
                })
                .unwrap_or_default();
            publish_startup_report(
                &this.svc,
                &this.parent_tab_id,
                &this.tab_id,
                &this.worker_title,
                &this.plan_id,
                &this.task_id,
                dispatch_ready_event(
                    &this.plan_id,
                    &this.task_id,
                    &this.tab_id,
                    &this.attempt_id,
                    &evidence,
                    &this.canonical_project_path,
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
                &this.svc,
                &this.parent_tab_id,
                &this.tab_id,
                &this.worker_title,
                &this.plan_id,
                &this.task_id,
                &this.attempt_id,
                this.spawn_command.clone(),
            );
            publish_action_needed_event(
                &this.svc,
                &this.parent_tab_id,
                &this.tab_id,
                &this.worker_title,
                startup_recovery_event(
                    &this.plan_id,
                    &this.task_id,
                    &this.tab_id,
                    &this.attempt_id,
                    Some(&this.startup_attempt_id),
                    &stage,
                ),
            );
            emit_plan_changed(&this.app, &this.plan_id);
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
                this.plan_id,
                this.task_id,
                this.tab_id,
                this.attempt_id,
                stage,
                reason
            );
            let attempt_finalized = if worker_may_be_alive {
                match this.svc.plan_store.mark_worker_startup_deadline(
                    &this.plan_id,
                    &this.task_id,
                    &this.tab_id,
                    &this.attempt_id,
                    true,
                    epoch_ms(),
                ) {
                    Ok(finalized) => finalized,
                    Err(error) => {
                        log::warn!(
                            "[task-dispatch] survivor finalization failed attempt={}: {error}",
                            this.attempt_id
                        );
                        false
                    }
                }
            } else {
                false
            };
            emit_plan_changed(&this.app, &this.plan_id);
            publish_startup_report(
                &this.svc,
                &this.parent_tab_id,
                &this.tab_id,
                &this.worker_title,
                &this.plan_id,
                &this.task_id,
                dispatch_startup_failed_event(
                    &this.plan_id,
                    &this.task_id,
                    &this.tab_id,
                    &this.attempt_id,
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
