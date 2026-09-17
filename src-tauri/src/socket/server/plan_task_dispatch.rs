use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tauri::AppHandle;

use crate::plan::{Task, TaskLaneMetadata};
use crate::service::plan::milestone_gates::{
    regression_dispatch_decision, DispatchExemption, RegressionDispatchDecision, RegressionGate,
};
use crate::service::plan::PlanError;
use crate::service::worker_ack::STARTUP_BINDING_DEADLINE;
use crate::service::{
    verify_worker_lane, LaneProtection, ProvisionedWorkerLane, ServiceState, SpawnAgentParams,
    WorkerLaneProvenance,
};

use super::plan::{emit_plan_changed, plan_rpc_err};
use super::plan_refs;
use super::JsonRpcError;

mod attempt_rollback;
mod brief;
pub(super) mod canonical_writer;
mod dispatch_failure;
mod gates;
mod lane_commit;
mod lane_resolution;
pub(super) mod lane_safety;
mod preflight;
mod routing;
mod workspace_admission;

use super::safety_nudge::{safety_incident_text, SAFETY_INCIDENT_KIND};
use crate::multiplexer::types::TabCloseReason;
use attempt_rollback::roll_back_dispatch_attempt;
use brief::build_brief_with_manifest;
#[cfg(test)]
pub(super) use brief::{build_brief, build_brief_with_personality};
use dispatch_failure::{
    cleanup_succeeded, dispatch_startup_failed_event, dispatch_wait_failed,
    DispatchFailureDisposition, DispatchWaitFailure,
};
use gates::dispatch_authorized_gates;
use lane_commit::{
    dispatch_ready_event, dispatch_response_lane, lane_metadata_for_dispatch, DispatchEvidence,
};
use lane_resolution::{
    checkout_required, invalid_dispatch, plan_dispatch_lane_placement, resolve_dispatch_lane,
    DispatchLanePlacement,
};
use lane_safety::{
    atomic_lane_commitment, dispatch_safety_nudges, preflight_checkout_occupancy,
    refuse_unsafe_mutating_write,
};
use preflight::{preflight_dispatch_task_status, preflight_review_capacity};
use routing::{
    dispatch_worker_identity, provider_mismatch_error, resolve_dispatch_routing,
    unqualified_model_error,
};
use workspace_admission::preflight_dispatch_workspace;

fn owned_str(p: Option<&Value>, key: &str) -> Option<String> {
    p.and_then(|x| x.get(key))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn dispatch_reasoning_effort(p: Option<&Value>) -> (Option<String>, Option<String>) {
    let requested = owned_str(p, "reasoning_effort");
    (requested.clone(), requested)
}

const DISPATCH_READY_TIMEOUT: Duration = STARTUP_BINDING_DEADLINE;

fn dispatch_failed(stage: &str, message: String) -> JsonRpcError {
    JsonRpcError {
        code: -32603,
        message: format!("dispatch failed at {stage}: {message}"),
        data: Some(json!({
            "type": "task_dispatch_failed",
            "stage": stage,
            "reason": message,
        })),
    }
}

fn verify_dispatched_task_binding(
    store: &crate::service::plan::PlanStore,
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
) -> Result<(), String> {
    match store.task_binding_for_tab(tab_id) {
        Some((bound_plan, bound_task)) if bound_plan == plan_id && bound_task == task_id => Ok(()),
        binding => Err(format!(
            "ORCHESTRATION PRODUCT INCIDENT: dispatched Worker tab '{tab_id}' has no matching \
             authoritative task binding. Impact: zero-flag reports cannot be attached safely. \
             Fallback: pass explicit --plan {plan_id} --task {task_id} and alert the Orchestrator; \
             no plan or task was guessed. Resolved binding: {binding:?}"
        )),
    }
}

fn task_launch_authority_for_dispatch(
    plan_id: &str,
    task_id: &str,
    attempt_id: &str,
    lane: &TaskLaneMetadata,
) -> crate::service::TaskLaunchAuthority {
    crate::service::TaskLaunchAuthority {
        plan_id: plan_id.to_string(),
        task_id: task_id.to_string(),
        attempt_id: attempt_id.to_string(),
        lane_base: lane.base.clone(),
    }
}

pub(super) fn await_agent_ready(
    ready_rx: tokio::sync::oneshot::Receiver<Result<(), String>>,
    deadline: Instant,
) -> Result<(), DispatchWaitFailure> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(DispatchWaitFailure::Uncertain(
            "agent readiness deadline expired".into(),
        ));
    }
    match tauri::async_runtime::block_on(
        async move { tokio::time::timeout(remaining, ready_rx).await },
    ) {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(error))) => Err(DispatchWaitFailure::Definitive(error)),
        // A dropped sender only means a newer generation superseded it; the worker may still be starting, so it is not failure evidence.
        Ok(Err(_)) => Err(DispatchWaitFailure::Uncertain(
            "agent readiness channel closed".into(),
        )),
        Err(_) => Err(DispatchWaitFailure::Uncertain(
            "agent readiness deadline expired".into(),
        )),
    }
}

pub(super) fn wait_for_tab_session(
    svc: &ServiceState,
    tab_id: &str,
    deadline: Instant,
) -> Result<String, DispatchWaitFailure> {
    crate::service::general_agent::binding_wait::wait_for_session_binding(deadline, || {
        svc.mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.agent_session_id.clone())
            .filter(|session| !session.trim().is_empty())
    })
    .ok_or_else(|| {
        if svc.mux.read().get_tab(tab_id).is_none() {
            // The tab is confirmed gone: no worker can be alive under it.
            DispatchWaitFailure::Definitive(format!(
                "spawned tab '{tab_id}' was removed before session binding"
            ))
        } else {
            DispatchWaitFailure::Uncertain(format!(
                "spawned tab '{tab_id}' did not bind a session before the deadline"
            ))
        }
    })
}

fn close_tab_if_present(svc: &ServiceState, tab_id: &str) -> Result<(), String> {
    if svc.mux.read().get_tab(tab_id).is_none() {
        Ok(())
    } else {
        svc.close_tab_with_reason(tab_id, TabCloseReason::SpawnRollback)
    }
}

fn apply_dispatch_starting_contract(
    result: &mut Value,
    tab_id: &str,
    spawn: &crate::service::SpawnResult,
) {
    let mut spawn_metadata = json!(spawn);
    if let Some(fields) = spawn_metadata.as_object_mut() {
        fields.remove("tab_id");
    }
    result["accepted"] = json!(true);
    result["state"] = json!("awaiting_dispatch_ready");
    result["readiness"] = json!({
        "ready": false,
        "awaiting_event": "dispatch_ready",
    });
    result["tab_id"] = json!(tab_id);
    result["spawn"] = spawn_metadata;
}

/// Decide placement first — occupancy only applies to a checkout the task
/// will actually land on, never a pre-provisioning placeholder — codeterm#343.
fn preflight_dispatch_placement(
    plan: &crate::plan::Plan,
    task_id: &str,
    spawn_cwd: &str,
    lane_protection: LaneProtection,
    will_provision_lane: bool,
) -> Result<(), JsonRpcError> {
    if will_provision_lane {
        return Ok(());
    }
    preflight_checkout_occupancy(plan, task_id, spawn_cwd, lane_protection)
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn record_lane_safety_incident(
    store: &crate::service::plan::PlanStore,
    plan_id: &str,
    task_id: &str,
    error: &JsonRpcError,
) {
    let Some(data) = error.data.as_ref() else {
        return;
    };
    // Only a lane-safety refusal is an incident; a checkout that simply did not
    // resolve carries no violation and must not be filed as one.
    let Some(violation_id) = data.get("violation_id").and_then(Value::as_str) else {
        return;
    };
    let reason = data
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let text = safety_incident_text(task_id, violation_id, reason);
    if let Err(error) = store.add_plan_stride(
        plan_id,
        SAFETY_INCIDENT_KIND,
        &text,
        crate::plan::AgentRef::from_tab("dispatcher"),
        epoch_ms(),
    ) {
        log::warn!("[lane-safety] incident evidence not recorded for {task_id}: {error}");
    }
}

pub(super) fn publish_startup_report(
    svc: &ServiceState,
    parent_tab_id: &str,
    worker_tab_id: &str,
    worker_title: &str,
    plan_id: &str,
    task_id: &str,
    payload: Value,
) {
    svc.publish_worker_lifecycle_report(
        parent_tab_id,
        worker_tab_id,
        worker_title,
        plan_id,
        task_id,
        payload,
    );
}

pub(super) fn arm_worker_ack_deadline(
    svc: &ServiceState,
    parent_tab_id: &str,
    worker_tab_id: &str,
    worker_title: &str,
    plan_id: &str,
    task_id: &str,
    attempt_id: &str,
    spawn_command: String,
) {
    svc.arm_worker_ack_deadline(
        parent_tab_id,
        worker_tab_id,
        worker_title,
        plan_id,
        task_id,
        attempt_id,
        spawn_command,
    );
}

#[cfg(test)]
fn worker_ack_timeout_event(
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
    attempt_id: &str,
    spawn_command: &str,
) -> Value {
    crate::service::worker_ack::worker_ack_timeout_event(
        plan_id,
        task_id,
        tab_id,
        attempt_id,
        spawn_command,
    )
}

pub(super) struct DispatchStartup {
    pub(super) app: AppHandle,
    pub(super) svc: ServiceState,
    pub(super) plan_id: String,
    pub(super) task_id: String,
    pub(super) tab_id: String,
    pub(super) attempt_id: String,
    pub(super) parent_tab_id: String,
    pub(super) worker_title: String,
    pub(super) original_task: Task,
    pub(super) original_plan_mode: crate::plan::PlanMode,
    pub(super) lane_for_commit: TaskLaneMetadata,
    pub(super) canonical_project_path: String,
    pub(super) provenance: Option<WorkerLaneProvenance>,
    pub(super) provisioned_lane: Option<ProvisionedWorkerLane>,
    pub(super) lane_protection: LaneProtection,
    pub(super) spawn_command: String,
}

impl DispatchStartup {
    pub(super) fn run(self, ready_rx: tokio::sync::oneshot::Receiver<Result<(), String>>) {
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
                    .map_err(|error| error.to_string())
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
                if worker_may_be_alive {
                    if let Err(error) = self
                        .svc
                        .deliver_worker_ack_reminder(&self.tab_id, &self.attempt_id)
                    {
                        log::warn!(
                            "[task-dispatch] survivor reminder failed attempt={}: {error}",
                            self.attempt_id
                        );
                    }
                }
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

/// Runs the dispatch readiness transaction and reacts to the outcome.
///
/// A stage failure only triggers the destructive `rollback` (task-snapshot
/// restore + tab close + lane cleanup) when it is a *definitive* failure. When
/// `agent_ready` or `session_binding` fail with evidence that is merely
/// absent (a deadline, or a channel closed by a superseded launch
/// generation), the worker may still be alive; the transaction surfaces the
/// error to the caller without touching the lane or the tab.
fn complete_dispatch_transaction<Ready, Binding, Provenance, Commit, Rollback>(
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

fn gate_keys(gates: &[RegressionGate]) -> String {
    gates
        .iter()
        .map(|g| g.key.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

// Refuse the dispatch loudly, naming every blocking gate — no tab is spawned.
fn regression_blocked_error(
    gates: Vec<RegressionGate>,
    retrospectives: Vec<RegressionGate>,
) -> JsonRpcError {
    let mut causes = Vec::new();
    if !gates.is_empty() {
        causes.push(format!(
            "pending regression gate(s) {}: run the warm integration QA (codeterm plan qa next <plan> --group <gate> --note \"<evidence>\") or record an authorized override (codeterm plan qa override <plan> --reason \"<why>\")",
            gate_keys(&gates)
        ));
    }
    if !retrospectives.is_empty() {
        causes.push(format!(
            "unrecorded group retrospective(s) {}: record each with codeterm plan groups close <plan> <group> --note \"<retrospective>\"",
            gate_keys(&retrospectives)
        ));
    }
    JsonRpcError {
        code: -32010,
        message: format!(
            "dispatch blocked by {}. Tag the task regression-gate/release-blocking only if it is the exception.",
            causes.join("; ")
        ),
        data: Some(json!({
            "type": "regression_gate_blocked",
            "blocking_gates": gates,
            "pending_retrospectives": retrospectives,
        })),
    }
}

/// Spawn a Worker and record runtime readiness without claiming ownership.
/// Failure before ACK restores plan state and closes the spawned tab.
pub(super) fn dispatch(
    params: &Option<Value>,
    app: &AppHandle,
    svc: &ServiceState,
    now: u64,
) -> Result<Value, JsonRpcError> {
    let store = &svc.plan_store;
    let p = params.as_ref();
    let authorized_gates = dispatch_authorized_gates(p)?;
    let (plan_id, task_id) = plan_refs::resolve_task(store, params)?;
    let detail = store
        .show_task(&task_id)
        .ok_or_else(|| plan_rpc_err(PlanError::TaskNotFound))?;
    preflight_dispatch_task_status(&plan_id, &detail.task)?;

    // Frontier policy: a non-blocker cannot spawn while regression gates are
    // pending. Decide BEFORE any tab is created so a block leaves no orphan.
    let plan = store
        .get(&plan_id)
        .ok_or_else(|| plan_rpc_err(PlanError::NotFound))?;
    svc.authorize_execution_action(
        &plan_id,
        &crate::service::AuthenticatedPlanCaller {
            tab_id: owned_str(p, "caller_tab_id"),
            session_id: owned_str(p, "session_id"),
            binding_nonce: owned_str(p, "session_binding_nonce"),
        },
    )
    .map_err(|message| JsonRpcError {
        code: -32003,
        message,
        data: Some(json!({ "type": "plan_orchestrator_authorization_required" })),
    })?;
    let mut exemption: Option<DispatchExemption> = None;
    match regression_dispatch_decision(&plan, &detail.task) {
        RegressionDispatchDecision::Clear => {}
        RegressionDispatchDecision::Exempt { exemption: e, .. } => exemption = Some(e),
        RegressionDispatchDecision::Blocked {
            blocking,
            retrospectives,
        } => {
            return Err(regression_blocked_error(blocking, retrospectives));
        }
    }
    // Same frontier policy for the review WIP gate: decide before any tab
    // exists. A task resuming after a bounce already holds its slot.
    if detail.task.status == crate::plan::TaskStatus::Pending {
        preflight_review_capacity(&plan, &detail.task).map_err(plan_rpc_err)?;
    }

    // Parent defaults to the dispatching (caller) tab, resolved server-side.
    let parent = owned_str(p, "parent_tab_id").or_else(|| owned_str(p, "caller_tab_id"));
    let caller_tab_id = owned_str(p, "caller_tab_id");
    let parent_tab_id = parent
        .clone()
        .or_else(|| plan.owner.tab_id.clone())
        .ok_or_else(|| invalid_dispatch("dispatch requires a parent Orchestrator tab"))?;
    preflight_dispatch_workspace(
        svc,
        owned_str(p, "workspace_id"),
        parent.as_deref(),
        caller_tab_id.as_deref(),
    )?;
    let routing = resolve_dispatch_routing(
        owned_str(p, "agent"),
        owned_str(p, "model"),
        detail.task.model.clone(),
        &svc.agent_registry.provider_ids(),
    );
    if let Some(error) = provider_mismatch_error(&routing) {
        return Err(error);
    }

    // Surface an unknown-model fallback loudly instead of silently downgrading.
    let resolved_provider = routing
        .agent
        .clone()
        .unwrap_or_else(crate::config::default_agent_provider);
    let execution_intent = match owned_str(p, "execution_intent").as_deref() {
        Some("read_only") => crate::plan::ExecutionIntent::ReadOnly,
        Some("mutating") => crate::plan::ExecutionIntent::Mutating,
        _ => {
            return Err(invalid_dispatch(
                "dispatch requires execution_intent read_only or mutating",
            ))
        }
    };
    let placement =
        canonical_writer::dispatch_worker_placement(p, &execution_intent, &detail.task)?;
    let allowance_capability = svc
        .agent_registry
        .with_provider(&resolved_provider, |provider| {
            provider.provider_allowance_capability()
        })
        .flatten();
    let reserve_pct = p
        .and_then(|value| value.get("provider_reserve_pct"))
        .and_then(Value::as_f64)
        .map(|value| value as f32)
        .or_else(|| allowance_capability.map(|capability| capability.default_reserve_pct))
        .unwrap_or(5.0);
    let allowance = if execution_intent == crate::plan::ExecutionIntent::Mutating {
        let allowance_capability = allowance_capability.ok_or_else(|| {
            invalid_dispatch("selected provider has no mutation allowance capability")
        })?;
        let snapshot = svc
            .usage_store
            .get_latest(&resolved_provider, None)
            .map_err(|error| invalid_dispatch(format!("provider allowance unavailable: {error}")))?
            .ok_or_else(|| invalid_dispatch("provider allowance telemetry is unknown"))?;
        Some(crate::plan::ProviderAllowance {
            remaining_pct: crate::usage::allowance_remaining_pct(&snapshot),
            observed_at: snapshot.captured_at.max(0) as u64,
            stale_after_ms: allowance_capability.telemetry_max_age_ms,
            reserve_pct,
        })
    } else {
        None
    };
    let mut warnings: Vec<String> = Vec::new();
    if allowance
        .as_ref()
        .is_some_and(|allowance| allowance.is_stale(now))
    {
        warnings.push(
            "provider allowance telemetry is stale; dispatch used the last known reserve value"
                .into(),
        );
    }
    crate::service::plan::interruption::provider_allowance_preflight(
        &execution_intent,
        allowance.as_ref(),
        now,
    )
    .map_err(plan_rpc_err)?;
    if let Some(notice) = crate::service::spawn::resolve_fresh_agent_model_with_notice(
        &svc.agent_registry,
        &resolved_provider,
        routing.model.clone(),
        None,
    )
    .fallback_notice
    {
        if let Some(error) = unqualified_model_error(&routing, &resolved_provider, &notice) {
            return Err(error);
        }
        warnings.push(format!(
            "requested model '{}' is not available for provider '{}'; spawning with '{}' instead",
            notice.requested_model, resolved_provider, notice.provider_resolved_model
        ));
    }

    // A personality is the executor persona: it names the tab and opens the brief.
    let (explicit_persona, title_provenance) = dispatch_worker_identity(&detail.task);
    let manifest = crate::service::plan::plan_skills::plan_skill_manifest(store, &plan_id)
        .map_err(plan_rpc_err)?;
    let brief = canonical_writer::compose_dispatch_brief(
        build_brief_with_manifest(
            &detail.task,
            &detail.sections,
            &plan_id,
            &manifest,
            &authorized_gates,
        ),
        placement,
    );

    let explicit_cwd = owned_str(p, "cwd");
    let canonical_cwd = if plan.canonical_project_path.trim().is_empty() {
        plan.project_path.clone()
    } else {
        plan.canonical_project_path.clone()
    };
    // Task-level intent decides whether a repository is involved at all; the
    // dispatch-time intent decides whether the mutating-work guards apply.
    let requires_repository = detail.task.execution_intent.requires_repository();
    let lane_protection = LaneProtection::for_dispatch_intent(&execution_intent);
    let lane_resolution = if requires_repository {
        if execution_intent == crate::plan::ExecutionIntent::ReadOnly {
            // A read-only Worker is the source-only rule's canonical exception.
            // Validate canonical as a checkout, but do not treat it as a lane.
            verify_worker_lane(
                &canonical_cwd,
                &canonical_cwd,
                None,
                None,
                LaneProtection::Waived,
            )
            .map_err(checkout_required)?;
            Some(lane_resolution::DispatchLaneResolution {
                spawn_cwd: canonical_cwd.clone(),
                existing: None,
            })
        } else if placement == crate::plan::WorkerPlacement::CanonicalWriter {
            canonical_writer::verify_canonical_writer_checkout(&canonical_cwd)?;
            Some(lane_resolution::DispatchLaneResolution {
                spawn_cwd: canonical_cwd.clone(),
                existing: None,
            })
        } else {
            let workspace_cwd = {
                let mux = svc.mux.read();
                let workspace_id = owned_str(p, "workspace_id");
                workspace_id
                    .as_deref()
                    .and_then(|id| mux.get_workspace(id))
                    .and_then(|workspace| workspace.cwd.clone())
                    .or_else(|| Some(canonical_cwd.clone()))
            };
            let caller_cwd = {
                let mux = svc.mux.read();
                caller_tab_id
                    .as_deref()
                    .and_then(|tab_id| mux.get_tab(tab_id))
                    .and_then(|tab| tab.cwd.clone())
            };
            Some(resolve_dispatch_lane(
                explicit_cwd.as_deref(),
                workspace_cwd.as_deref(),
                caller_cwd.as_deref(),
                &detail.task.lane,
                lane_protection,
            )?)
        }
    } else {
        None
    };
    // Placement decides lane-vs-canonical before a tab exists, so a refusal and
    // a provisioning both happen with nothing yet to clean up.
    let lane_placement = match lane_resolution.as_ref() {
        Some(resolution)
            if execution_intent == crate::plan::ExecutionIntent::Mutating
                && placement.is_isolated_lane() =>
        {
            let resolved = resolution.existing.clone().ok_or_else(|| {
                checkout_required("mutating dispatch resolved no repository checkout")
            })?;
            Some(
                plan_dispatch_lane_placement(&plan, &task_id, &detail.task.lane, resolved)
                    .inspect_err(|error| {
                        record_lane_safety_incident(store, &plan_id, &task_id, error)
                    })?,
            )
        }
        _ => None,
    };
    let will_provision_lane = lane_placement
        .as_ref()
        .is_some_and(DispatchLanePlacement::provisions);
    let spawn_cwd = match lane_placement.as_ref() {
        Some(placement) => placement.spawn_cwd().to_string(),
        None => lane_resolution
            .as_ref()
            .map(|lane| lane.spawn_cwd.clone())
            .unwrap_or_else(|| {
                explicit_cwd
                    .clone()
                    .unwrap_or_else(|| canonical_cwd.clone())
            }),
    };
    if requires_repository {
        preflight_dispatch_placement(
            &plan,
            &task_id,
            &spawn_cwd,
            lane_protection,
            will_provision_lane,
        )?;
    }
    let (planned_reasoning_effort, reasoning_effort) = dispatch_reasoning_effort(p);
    let mut spawn_params = SpawnAgentParams {
        agent: routing.agent.clone(),
        cwd: Some(spawn_cwd),
        title: explicit_persona,
        title_provenance,
        role: Some("Worker".into()),
        role_profile: crate::service::role_profile::builtin_worker_profile(
            detail.task.executor_role.as_deref(),
        ),
        parent_tab_id: parent,
        plan_id: Some(plan_id.clone()),
        task_id: Some(task_id.clone()),
        workspace_id: owned_str(p, "workspace_id"),
        model: routing.model.clone(),
        planned_model: detail.task.model.clone(),
        planned_reasoning_effort,
        reasoning_effort,
        spawned_by_tab_id: owned_str(p, "caller_tab_id"),
        task: Some(brief),
        ..Default::default()
    };

    let lane_label = format!("{plan_id}-{task_id}");
    let provisioned_lane = match lane_placement.as_ref() {
        Some(DispatchLanePlacement::Provision { base_checkout }) => Some(
            svc.provision_dispatch_worker_lane(&mut spawn_params, base_checkout, &lane_label)
                .map_err(|error| dispatch_failed("lane_provision", error))?,
        ),
        // A lane we created on an earlier dispatch is still ours. Without this the
        // reused worktree carries no provenance and the trust dialog waits forever.
        Some(DispatchLanePlacement::Existing(provenance)) => {
            spawn_params.provisioned_cwd = Some(provenance.worktree_path.clone());
            None
        }
        _ => None,
    };
    let initial_provenance = if execution_intent == crate::plan::ExecutionIntent::Mutating
        && requires_repository
        && placement.is_isolated_lane()
    {
        match provisioned_lane.as_ref() {
            Some(lane) => lane.verify(),
            None => match lane_placement.as_ref() {
                Some(DispatchLanePlacement::Existing(provenance)) => Ok(provenance.clone()),
                _ => Err("dispatch did not provision or resolve a worker lane".to_string()),
            },
        }
        .map(Some)
    } else {
        Ok(None)
    };
    let initial_provenance = match initial_provenance {
        Ok(provenance) => provenance,
        Err(error) => {
            if let Some(lane) = provisioned_lane {
                if let Err(cleanup) = lane.cleanup() {
                    return Err(dispatch_failed(
                        "compensation",
                        format!("{error}; lane cleanup failed: {cleanup}"),
                    ));
                }
            }
            return Err(dispatch_failed("provenance_validation", error));
        }
    };
    // A provisioned lane skipped the placement check and no tab exists yet, so
    // the red line is asked here of the lane the Worker will actually hold.
    if let Some(provenance) = initial_provenance.as_ref() {
        if let Err(error) = refuse_unsafe_mutating_write(
            &plan,
            &task_id,
            &provenance.worktree_path,
            execution_intent.clone(),
        ) {
            record_lane_safety_incident(store, &plan_id, &task_id, &error);
            if let Some(lane) = provisioned_lane {
                if let Err(cleanup) = lane.cleanup() {
                    return Err(dispatch_failed(
                        "compensation",
                        format!("{}; lane cleanup failed: {cleanup}", error.message),
                    ));
                }
            }
            return Err(error);
        }
    }
    let original_task = detail.task.clone();
    let original_plan_mode = plan.mode.clone();
    let mut lane_for_commit = detail.task.lane.clone();
    if let Some((workspace_id, workspace_name)) =
        svc.resolve_spawn_workspace_identity(&spawn_params)
    {
        lane_for_commit.workspace_id.get_or_insert(workspace_id);
        lane_for_commit.workspace_name.get_or_insert(workspace_name);
    }
    let starting_lane = initial_provenance
        .as_ref()
        .map(|provenance| lane_metadata_for_dispatch(&lane_for_commit, provenance, &canonical_cwd))
        .unwrap_or_else(|| {
            let mut lane = lane_for_commit.clone();
            lane.worktree_path = None;
            lane.branch = None;
            lane.base = None;
            lane.cleanup_status = None;
            lane
        });
    // Team identity rides the same committed value as the isolated lane, so no
    // persisted state carries a workspace without the worktree that isolates it.
    let starting_lane = atomic_lane_commitment(&starting_lane);
    let reserved_attempt_id = crate::plan::ids::short_id("attempt");
    spawn_params.task_launch_authority = Some(task_launch_authority_for_dispatch(
        &plan_id,
        &task_id,
        &reserved_attempt_id,
        &starting_lane,
    ));
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let spawn = match svc.spawn_agent_sync_with_ready(spawn_params, Some(ready_tx)) {
        Ok(spawn) => spawn,
        Err(error) => {
            if let Some(lane) = provisioned_lane {
                if let Err(cleanup) = lane.cleanup() {
                    return Err(dispatch_failed(
                        "compensation",
                        format!("{error}; lane cleanup failed: {cleanup}"),
                    ));
                }
            }
            return Err(dispatch_failed("agent_spawn", error));
        }
    };
    let tab_id = spawn.tab_id.clone();
    let attempt_id = match store.begin_task_dispatch_with_reserved_attempt_and_gates(
        &plan_id,
        &task_id,
        &tab_id,
        &spawn.title,
        starting_lane,
        execution_intent,
        allowance,
        Some(&resolved_provider),
        &reserved_attempt_id,
        authorized_gates.clone(),
        placement,
        now,
    ) {
        Ok(attempt_id) => attempt_id,
        Err(error) => {
            let cleanup = rollback_failed_dispatch(
                provisioned_lane,
                || Ok(()),
                || close_tab_if_present(svc, &tab_id),
            );
            emit_plan_changed(app, &plan_id);
            return Err(dispatch_failed(
                "task_start",
                match cleanup {
                    Ok(()) => error.to_string(),
                    Err(cleanup) => format!("{error}; {cleanup}"),
                },
            ));
        }
    };
    if let Err(error) =
        store.set_attempt_plan_skill_manifest(&plan_id, &task_id, &attempt_id, manifest, now)
    {
        let cleanup = rollback_failed_dispatch(
            provisioned_lane,
            || {
                store
                    .revert_task_dispatch(&plan_id, &task_id, now)
                    .map_err(|error| error.to_string())
            },
            || close_tab_if_present(svc, &tab_id),
        );
        return Err(dispatch_failed(
            "skill_manifest",
            match cleanup {
                Ok(()) => error.to_string(),
                Err(cleanup) => format!("{error}; {cleanup}"),
            },
        ));
    }
    if let Err(error) = verify_dispatched_task_binding(store, &plan_id, &task_id, &tab_id) {
        let cleanup = roll_back_dispatch_attempt(
            store,
            &plan_id,
            &original_task,
            &original_plan_mode,
            &tab_id,
            &attempt_id,
            provisioned_lane,
            || close_tab_if_present(svc, &tab_id),
            now,
        );
        emit_plan_changed(app, &plan_id);
        return Err(dispatch_failed(
            "task_binding",
            match cleanup {
                Ok(()) => error,
                Err(cleanup) => format!("{error}; {cleanup}"),
            },
        ));
    }
    emit_plan_changed(app, &plan_id);
    let startup = DispatchStartup {
        app: app.clone(),
        svc: svc.clone(),
        plan_id: plan_id.clone(),
        task_id: task_id.clone(),
        tab_id: tab_id.clone(),
        attempt_id: attempt_id.clone(),
        parent_tab_id,
        worker_title: spawn.title.clone(),
        original_task,
        original_plan_mode,
        lane_for_commit,
        canonical_project_path: canonical_cwd.clone(),
        provenance: initial_provenance.clone(),
        provisioned_lane,
        lane_protection,
        spawn_command: format!("codeterm plan task dispatch {plan_id} {task_id}"),
    };
    std::thread::spawn(move || startup.run(ready_rx));

    let model_fallback = spawn.model_fallback.clone();
    let mut result = json!({
        "ok": true,
        "plan_id": plan_id,
        "task_id": task_id,
        "attempt_id": attempt_id,
        "agent": resolved_provider,
        "authorized_gates": authorized_gates,
        "placement": placement,
    });
    if let Some(provenance) = initial_provenance.as_ref() {
        result["lane"] = dispatch_response_lane(provenance, &canonical_cwd, &detail.task.lane);
    }
    apply_dispatch_starting_contract(&mut result, &tab_id, &spawn);
    if !warnings.is_empty() {
        result["warnings"] = json!(warnings);
    }
    if let Some(exemption) = exemption {
        result["regression_exception"] = json!({
            "exemption": exemption,
            "reason": exemption.reason(),
        });
    }
    if let Some(model_fallback) = model_fallback {
        result["model_fallback"] = json!(model_fallback);
    }
    Ok(result)
}

fn rollback_failed_dispatch<R, C>(
    lane: Option<ProvisionedWorkerLane>,
    revert_task: R,
    close_tab: C,
) -> Result<(), String>
where
    R: FnOnce() -> Result<(), String>,
    C: FnOnce() -> Result<(), String>,
{
    let mut errors = Vec::new();
    if let Err(error) = revert_task() {
        errors.push(format!("task restore failed: {error}"));
    }
    if let Err(error) = close_tab() {
        errors.push(format!("Tab close failed: {error}"));
    }
    if let Some(lane) = lane {
        if let Err(error) = lane.cleanup() {
            errors.push(format!("lane cleanup failed: {error}"));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(test)]
mod tests;
