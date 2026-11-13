use codeterm_service_core::plan::TaskLaneMetadata;
use codeterm_service_core::service::plan::PlanError;
use codeterm_service_core::service::worker_ack::STARTUP_BINDING_DEADLINE;
use codeterm_service_core::service::{
    LaneProtection, ProvisionedWorkerLane, ServiceState, SpawnAgentParams,
};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

use super::plan::{emit_plan_changed, plan_rpc_err};
use super::plan_refs;
use super::JsonRpcError;

mod attempt_rollback;
mod brief;
pub(super) mod canonical_writer;
mod dispatch_failure;
mod dispatch_helpers;
mod gates;
mod lane_commit;
mod lane_resolution;
pub(super) mod lane_safety;
mod preflight;
mod recovery;
pub(super) mod remote;
pub(crate) mod remote_recovery;
mod role_params;
mod routing;
pub(crate) mod startup;
mod tab_reservation;
pub(super) mod todo_seed;
mod unplanned;
mod workspace_admission;

use super::safety_nudge::{safety_incident_text, SAFETY_INCIDENT_KIND};
use attempt_rollback::roll_back_dispatch_attempt;
use brief::build_brief_for_capabilities;
#[cfg(test)]
pub(super) use brief::{build_brief, build_brief_with_manifest, build_brief_with_personality};
use dispatch_failure::{
    cleanup_succeeded, dispatch_startup_failed_event, dispatch_wait_failed,
    DispatchFailureDisposition, DispatchWaitFailure,
};
use dispatch_helpers::finish_unreserved_dispatch;
pub(super) use dispatch_helpers::{
    dispatch_reasoning_effort, owned_str, respawn_worker_params, set_local_task_name_description,
    task_tab_description,
};
use dispatch_helpers::{rollback_failed_dispatch, verify_dispatched_task_binding};
use gates::dispatch_authorized_gates;
use lane_commit::{
    dispatch_ready_event, dispatch_response_lane, lane_metadata_for_dispatch, DispatchEvidence,
};
#[cfg(test)]
use lane_resolution::plan_dispatch_lane_placement;
use lane_resolution::{
    checkout_required, invalid_dispatch, refuse_missing_explicit_cwd, resolve_dispatch_lane,
    DispatchLanePlacement,
};
#[cfg(test)]
use lane_safety::preflight_checkout_occupancy;
use lane_safety::{atomic_lane_commitment, dispatch_safety_nudges};
#[cfg(test)]
use preflight::regression_dispatch_warning;
#[cfg(test)]
use preflight::{preflight_dispatch_placement, preflight_review_capacity};
use preflight::{preflight_dispatch_task_status, selected_regression_dispatch_warning};
pub(super) use recovery::publish_action_needed_event;
use recovery::startup_recovery_event;
use routing::dispatch_worker_identity;
pub(super) use tab_reservation::dispatch_startup_owner_is_live;
use tab_reservation::{
    close_tab_if_present, refuse_live_dispatch_startup_owner, DispatchReservationTab,
};
pub(crate) use unplanned::dispatch_unplanned;
use workspace_admission::preflight_dispatch_workspace;

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

fn task_launch_authority_for_dispatch(
    plan_id: &str,
    task_id: &str,
    attempt_id: &str,
    lane: &TaskLaneMetadata,
) -> codeterm_service_core::service::TaskLaunchAuthority {
    codeterm_service_core::service::TaskLaunchAuthority {
        plan_id: plan_id.to_string(),
        task_id: task_id.to_string(),
        attempt_id: attempt_id.to_string(),
        lane_base: lane.base.clone(),
    }
}

fn set_dispatch_task_brief(
    spawn_params: &mut SpawnAgentParams,
    brief: String,
    manifest: &[codeterm_service_core::plan::PlanSkillManifestEntry],
) {
    spawn_params.task = Some(brief);
    spawn_params.plan_skill_manifest = Some(manifest.to_vec());
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
    match codeterm_service_core::async_runtime::block_on(async move {
        tokio::time::timeout(remaining, ready_rx).await
    }) {
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
    codeterm_service_core::service::general_agent::binding_wait::wait_for_session_binding(
        deadline,
        || {
            svc.mux
                .read()
                .get_tab(tab_id)
                .and_then(|tab| tab.agent_session_id.clone())
                .filter(|session| !session.trim().is_empty())
        },
    )
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

fn apply_dispatch_starting_contract(
    result: &mut Value,
    tab_id: &str,
    spawn: &codeterm_service_core::service::SpawnResult,
) {
    let mut spawn_metadata = json!(spawn);
    if let Some(fields) = spawn_metadata.as_object_mut() {
        fields.remove("tab_id");
    }
    // A spawn is not acceptance: `dispatch_ready` arrives once the session
    // binds, so the response names the spawn and the event still owed.
    result["accepted"] = json!(false);
    result["spawned"] = json!(true);
    result["state"] = json!("awaiting_worker_ack");
    result["readiness"] = json!({
        "ready": false,
        "awaiting_event": "worker_ack",
    });
    result["tab_id"] = json!(tab_id);
    result["spawn"] = spawn_metadata;
}

/// An uncertain readiness or binding result may leave the Worker alive.
/// Preserve it for exact-tab recovery instead of finalizing on missing evidence.
fn startup_wait_is_recoverable(error: &JsonRpcError) -> bool {
    let data = error.data.as_ref();
    let stage = data
        .and_then(|data| data.get("stage"))
        .and_then(Value::as_str);
    let worker_may_be_alive = data
        .and_then(|data| data.get("worker_may_be_alive"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    matches!(stage, Some("agent_ready" | "session_binding")) && worker_may_be_alive
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn record_lane_safety_incident(
    store: &codeterm_service_core::service::plan::PlanStore,
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
        codeterm_service_core::plan::AgentRef::from_tab("dispatcher"),
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
    codeterm_service_core::service::worker_ack::worker_ack_timeout_event(
        plan_id,
        task_id,
        tab_id,
        attempt_id,
        spawn_command,
    )
}

#[cfg(test)]
use startup::complete_dispatch_transaction;
pub(crate) use startup::DispatchStartup;

/// Spawn a Worker and record runtime readiness without claiming ownership.
/// Failure before ACK restores plan state and closes the spawned tab.
pub(super) fn dispatch(
    params: &Option<Value>,
    svc: &ServiceState,
    now: u64,
) -> Result<Value, JsonRpcError> {
    dispatch_with_spawn(
        params,
        svc,
        now,
        |svc, spawn_params, ready_tx| svc.spawn_agent_sync_with_ready(spawn_params, ready_tx),
        || {},
    )
}

pub(super) fn dispatch_with_spawn<S, O>(
    params: &Option<Value>,
    svc: &ServiceState,
    now: u64,
    spawn_agent: S,
    after_startup_owner_observation: O,
) -> Result<Value, JsonRpcError>
where
    S: FnOnce(
        &ServiceState,
        SpawnAgentParams,
        Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    ) -> Result<codeterm_service_core::service::SpawnResult, String>,
    O: FnOnce(),
{
    if params
        .as_ref()
        .and_then(|value| value.get("mesh_peer"))
        .and_then(Value::as_str)
        .is_some_and(|peer| !peer.trim().is_empty())
    {
        return remote::dispatch(params, svc, now);
    }
    dispatch_with_spawn_approval_exemption(
        params,
        svc,
        now,
        None,
        spawn_agent,
        after_startup_owner_observation,
    )
}

fn dispatch_with_spawn_approval_exemption<S, O>(
    params: &Option<Value>,
    svc: &ServiceState,
    now: u64,
    approval_exempt_task_id: Option<&str>,
    spawn_agent: S,
    after_startup_owner_observation: O,
) -> Result<Value, JsonRpcError>
where
    S: FnOnce(
        &ServiceState,
        SpawnAgentParams,
        Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    ) -> Result<codeterm_service_core::service::SpawnResult, String>,
    O: FnOnce(),
{
    let store = &svc.plan_store;
    let p = params.as_ref();
    let authorized_gates = dispatch_authorized_gates(p)?;
    refuse_missing_explicit_cwd(owned_str(p, "cwd").as_deref())?;
    let (plan_id, task_id) = plan_refs::resolve_task(store, params)?;
    let initial = store
        .task_dispatch_preflight_projection(&plan_id, &task_id)
        .map_err(plan_rpc_err)?;
    let description_override =
        owned_str(p, "description").filter(|description| !description.trim().is_empty());
    let worker_description = task_tab_description(&initial.name, description_override.as_deref());
    let worker_description =
        codeterm_service_core::service::normalize_tab_description(&worker_description)
            .map_err(invalid_dispatch)?
            .ok_or_else(|| invalid_dispatch("task Worker description cannot be empty"))?;
    preflight_dispatch_task_status(&plan_id, &initial)?;

    // Group records stay visible without taking the dispatch path away.
    if !store
        .plan_exists_projection(&plan_id)
        .map_err(plan_rpc_err)?
    {
        return Err(plan_rpc_err(PlanError::NotFound));
    }
    let caller = codeterm_service_core::service::AuthenticatedPlanCaller {
        tab_id: owned_str(p, "caller_tab_id"),
        session_id: owned_str(p, "session_id"),
        binding_nonce: owned_str(p, "session_binding_nonce"),
    };
    let unplanned_dispatch =
        approval_exempt_task_id == Some(task_id.as_str()) && initial.is_unplanned_work;
    (if unplanned_dispatch {
        svc.authorize_plan_orchestrator(&plan_id, &caller)
    } else {
        svc.authorize_execution_action(&plan_id, &caller)
    })
    .map_err(|error| {
        crate::server::plan_task_authority::execution_authority_error(
            error,
            Some("plan_orchestrator_authorization_required"),
        )
    })?;
    remote_recovery::recover_remote_attempts(svc, &plan_id, &task_id, now, false)?;
    let task = store
        .task_dispatch_preflight_projection(&plan_id, &task_id)
        .map_err(plan_rpc_err)?;
    let preparation = store
        .worker_task_preparation_projection(&plan_id, &task_id)
        .map_err(plan_rpc_err)?;
    let lane_facts = store
        .task_lane_admission_projection(&plan_id, &task_id)
        .map_err(plan_rpc_err)?;
    let classify_lane = |path: &str| {
        codeterm_service_core::service::task_lane_guard::classify_lane_facts(
            &lane_facts.canonical_path,
            path,
            lane_facts
                .foreign_lanes
                .iter()
                .map(|lane| (lane.task_id.as_str(), lane.worktree_path.as_str())),
        )
    };
    let seed = store
        .task_dispatch_seed_projection(&plan_id, &task_id, "")
        .map_err(plan_rpc_err)?;
    refuse_live_dispatch_startup_owner(store, svc, &plan_id, &task_id)?;
    let regression_warning = selected_regression_dispatch_warning(store, &plan_id)?;
    if matches!(
        task.status,
        codeterm_service_core::plan::TaskStatus::Pending
            | codeterm_service_core::plan::TaskStatus::Blocked
    ) {
        store
            .dispatch_review_capacity_warning_projection(&plan_id, &task_id)
            .map_err(plan_rpc_err)?;
    }

    // Parent defaults to the dispatching (caller) tab, resolved server-side.
    let parent = owned_str(p, "parent_tab_id").or_else(|| owned_str(p, "caller_tab_id"));
    let caller_tab_id = owned_str(p, "caller_tab_id");
    let parent_tab_id = parent
        .clone()
        .or_else(|| task.owner_tab_id.clone())
        .ok_or_else(|| invalid_dispatch("dispatch requires a parent Orchestrator tab"))?;
    preflight_dispatch_workspace(
        svc,
        owned_str(p, "workspace_id"),
        parent.as_deref(),
        caller_tab_id.as_deref(),
    )?;
    let role = role_params::resolve(
        svc,
        p,
        &parent_tab_id,
        preparation.brief.executor_role.as_deref(),
    )
    .map_err(invalid_dispatch)?;
    let requested_model = owned_str(p, "model")
        .or_else(|| task.model.clone())
        .or(role.model);
    let model_selection = codeterm_service_core::service::spawn::resolve_agent_model_selection(
        &svc.agent_registry,
        owned_str(p, "agent").as_deref(),
        requested_model.as_deref(),
        &svc.configuration().general_agent.agent_provider,
    )
    .map_err(super::model_errors::invalid_model_rpc_error)?;
    let resolved_provider = model_selection.provider_id;
    let selected_model = model_selection.model;
    let execution_intent = match owned_str(p, "execution_intent").as_deref() {
        Some("read_only") => codeterm_service_core::plan::ExecutionIntent::ReadOnly,
        Some("mutating") => codeterm_service_core::plan::ExecutionIntent::Mutating,
        _ => {
            return Err(invalid_dispatch(
                "dispatch requires execution_intent read_only or mutating",
            ))
        }
    };
    let without_git = canonical_writer::dispatch_without_git(&execution_intent, &task)?;
    let (placement, placement_reason) = canonical_writer::resolve_placement(
        p,
        &execution_intent,
        &task,
        &store
            .checkout_peer_facts(if task.canonical_project_path.trim().is_empty() {
                &task.project_path
            } else {
                &task.canonical_project_path
            })
            .map_err(plan_rpc_err)?,
        without_git,
    )?;
    let worker_profile = role.profile;
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
        .unwrap_or(0.0);
    let allowance = if execution_intent == codeterm_service_core::plan::ExecutionIntent::Mutating {
        match (
            allowance_capability,
            svc.usage_store.get_latest(&resolved_provider, None),
        ) {
            (Some(capability), Ok(Some(snapshot))) => {
                Some(codeterm_service_core::plan::ProviderAllowance {
                    remaining_pct: codeterm_service_core::usage::allowance_remaining_pct(&snapshot),
                    observed_at: snapshot.captured_at.max(0) as u64,
                    stale_after_ms: capability.telemetry_max_age_ms,
                    reserve_pct,
                })
            }
            _ => None,
        }
    } else {
        None
    };
    let mut warnings: Vec<String> = Vec::new();
    if let Some(warning) = regression_warning {
        warnings.push(warning);
    }
    if allowance
        .as_ref()
        .is_some_and(|allowance| allowance.is_stale(now))
    {
        warnings.push(
            "provider allowance telemetry is stale; dispatch used the last known reserve value"
                .into(),
        );
    }
    if let Some(warning) =
        codeterm_service_core::service::plan::interruption::provider_allowance_warning(
            &execution_intent,
            allowance.as_ref(),
            &resolved_provider,
        )
    {
        warnings.push(warning);
    }
    let model_resolution =
        codeterm_service_core::service::spawn::resolve_fresh_agent_model_with_notice(
            &svc.configuration(),
            &svc.agent_registry,
            &resolved_provider,
            selected_model.clone(),
            None,
        );
    let dispatched_model = model_resolution.model.clone();
    let dispatched_provider_resolved_model = model_resolution.provider_resolved_model.clone();
    if let Some(warning) = model_resolution.warning.clone() {
        warnings.push(warning);
    }
    // A personality is the executor persona: it names the tab and opens the brief.
    let (explicit_persona, title_provenance) =
        dispatch_worker_identity(preparation.personality.as_deref());
    let manifest =
        codeterm_service_core::service::plan::plan_skills::plan_skill_manifest(store, &plan_id)
            .map_err(plan_rpc_err)?;

    let explicit_cwd = owned_str(p, "cwd");
    let canonical_cwd = if task.canonical_project_path.trim().is_empty() {
        task.project_path.clone()
    } else {
        task.canonical_project_path.clone()
    };
    // Task-level intent decides whether a repository is involved at all; the
    // dispatch-time intent decides whether the mutating-work guards apply.
    let requires_repository = task.execution_intent.requires_repository();
    let lane_protection = LaneProtection::for_dispatch_intent(&execution_intent);
    let lane_resolution =
        if execution_intent == codeterm_service_core::plan::ExecutionIntent::ReadOnly {
            Some(lane_resolution::DispatchLaneResolution {
                spawn_cwd: explicit_cwd
                    .clone()
                    .unwrap_or_else(|| canonical_cwd.clone()),
                existing: None,
            })
        } else if requires_repository {
            if !placement.is_isolated_lane() {
                if !without_git {
                    canonical_writer::verify_canonical_writer_checkout(&canonical_cwd)?;
                }
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
                        .or_else(|| Some(task.project_path.clone()))
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
                    &task.lane,
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
            if execution_intent == codeterm_service_core::plan::ExecutionIntent::Mutating
                && placement.is_isolated_lane() =>
        {
            let resolved = resolution.existing.clone().ok_or_else(|| {
                checkout_required("mutating dispatch resolved no repository checkout")
            })?;
            Some(
                lane_resolution::plan_dispatch_lane_placement_facts(
                    &plan_id,
                    &task.project_path,
                    &task.canonical_project_path,
                    &task_id,
                    &task.lane,
                    resolved,
                    &classify_lane,
                )
                .inspect_err(|error| {
                    record_lane_safety_incident(store, &plan_id, &task_id, error)
                })?,
            )
        }
        _ => None,
    };
    let replaces_recorded_lane = lane_placement
        .as_ref()
        .is_some_and(DispatchLanePlacement::provisions)
        && (task.lane.worktree_path.is_some() || task.lane.branch.is_some());
    let brief = canonical_writer::compose_dispatch_brief(
        build_brief_for_capabilities(
            svc.agent_registry
                .with_provider(&resolved_provider, |provider| provider.is_tool_less())
                .unwrap_or(false),
            (&preparation, &task),
            &plan_id,
            &manifest,
            &authorized_gates,
        ),
        placement,
        without_git,
    );
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
    if requires_repository && placement != codeterm_service_core::plan::WorkerPlacement::Shared {
        preflight::preflight_dispatch_checkout(
            store,
            svc,
            &plan_id,
            &task_id,
            &spawn_cwd,
            lane_protection,
            will_provision_lane,
        )?;
    }
    let (planned_reasoning_effort, reasoning_effort) = dispatch_reasoning_effort(p);
    let dispatched_reasoning_effort = reasoning_effort.clone();
    let mut spawn_params = SpawnAgentParams {
        agent: Some(resolved_provider.clone()),
        cwd: Some(spawn_cwd),
        title: explicit_persona,
        title_provenance,
        role: Some("Worker".into()),
        role_profile: worker_profile,
        parent_tab_id: Some(parent_tab_id.clone()),
        plan_id: Some(plan_id.clone()),
        task_id: Some(task_id.clone()),
        execution_intent: Some(execution_intent.clone()),
        sandbox: owned_str(p, "sandbox"),
        authorized_gates: authorized_gates.clone(),
        provider_reserve_pct: Some(reserve_pct),
        workspace_id: owned_str(p, "workspace_id"),
        model: selected_model.clone(),
        launch_profile: owned_str(p, "launch_profile"),
        planned_model: task.model.clone(),
        speed: owned_str(p, "speed").or_else(|| preparation.speed.clone()),
        planned_reasoning_effort,
        reasoning_effort,
        description: Some(worker_description),
        task_description_managed: description_override.is_none(),
        spawned_by_tab_id: owned_str(p, "caller_tab_id"),
        task: None,
        ..Default::default()
    };
    svc.validate_spawn_options(&resolved_provider, &spawn_params)
        .map_err(super::model_errors::spawn_target_rpc_error)?;
    let launch_profile = spawn_params.launch_profile.clone();
    let spawn_params_speed = spawn_params.speed.clone();
    set_dispatch_task_brief(&mut spawn_params, brief, &manifest);

    refuse_live_dispatch_startup_owner(store, svc, &plan_id, &task_id)?;

    let reserved_attempt_id = codeterm_service_core::plan::ids::short_id("attempt");
    let reservation_workspace = spawn_params
        .workspace_id
        .clone()
        .or_else(|| svc.mux.read().find_workspace_for_tab(&parent_tab_id))
        .ok_or_else(|| invalid_dispatch("dispatch could not resolve a workspace for its Worker"))?;
    let reserved_tab_id = {
        let mut mux = svc.mux.write();
        let tab_id = mux
            .create_tab(&reservation_workspace)
            .map_err(|error| dispatch_failed("tab_reservation", error))?;
        if let Some(title) = spawn_params.title.as_deref() {
            if let Err(error) = mux.rename_tab(&tab_id, title) {
                mux.rollback_spawn_tab(&reservation_workspace, &tab_id);
                return Err(dispatch_failed("tab_reservation", error));
            }
        }
        tab_id
    };
    let mut reservation_tab = DispatchReservationTab::new(svc, reserved_tab_id.clone());
    codeterm_service_core::startup::boot_timing::bind_request(&reserved_tab_id);
    let observed_owner = store
        .dispatch_startup_owner_candidate(&plan_id, &task_id)
        .map_err(|error| {
            let mut refusal = plan_rpc_err(error);
            if let Err(cleanup) = reservation_tab.close() {
                refusal.message = format!(
                    "{}; losing Worker tab cleanup failed: {cleanup}",
                    refusal.message
                );
            }
            refusal
        })?;
    let observed_owner_is_live = observed_owner.as_ref().is_some_and(|owner| {
        dispatch_startup_owner_is_live(svc, &plan_id, &task_id, &owner.tab_id)
    });
    after_startup_owner_observation();
    let dispatch_reservation = match store.reserve_task_dispatch_startup(
        &plan_id,
        &task_id,
        &reserved_attempt_id,
        &reserved_tab_id,
        observed_owner.clone(),
        observed_owner_is_live,
    ) {
        Ok(reservation) => reservation,
        Err(error) => {
            let mut refusal = plan_rpc_err(error);
            if let Err(cleanup) = reservation_tab.close() {
                refusal.message = format!(
                    "{}; losing Worker tab cleanup failed: {cleanup}",
                    refusal.message
                );
            }
            return Err(refusal);
        }
    };
    spawn_params.existing_tab_id = Some(reserved_tab_id.clone());

    let lane_label = format!("{plan_id}-{task_id}");
    let provisioned_lane = match lane_placement.as_ref() {
        Some(DispatchLanePlacement::Provision { base_checkout }) => Some(
            svc.provision_dispatch_worker_lane(
                &mut spawn_params,
                base_checkout,
                &lane_label,
                task.lane.base.as_deref(),
            )
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
    let initial_provenance = if execution_intent
        == codeterm_service_core::plan::ExecutionIntent::Mutating
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
        if let Err(error) = lane_safety::refuse_unsafe_mutating_lane(
            &plan_id,
            &task_id,
            if execution_intent == codeterm_service_core::plan::ExecutionIntent::Mutating {
                classify_lane(&provenance.worktree_path)
            } else {
                None
            },
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
    if let Err(mut refusal) = refuse_live_dispatch_startup_owner(store, svc, &plan_id, &task_id) {
        if let Err(cleanup) = rollback_failed_dispatch(provisioned_lane, || Ok(()), || Ok(())) {
            refusal.message = format!(
                "{}; losing Worker lane cleanup failed: {cleanup}",
                refusal.message
            );
        }
        return Err(refusal);
    }
    let mut lane_for_commit = task.lane.clone();
    if replaces_recorded_lane {
        lane_commit::clear_replaced_lane_routing_fields(&mut lane_for_commit);
    }
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
    spawn_params.task_launch_authority = Some(task_launch_authority_for_dispatch(
        &plan_id,
        &task_id,
        &reserved_attempt_id,
        &starting_lane,
    ));
    let attempt_start = store
        .dispatch_startup_owner_candidate(&plan_id, &task_id)
        .and_then(|begin_owner| {
            let begin_owner_is_live = begin_owner.as_ref().is_some_and(|owner| {
                dispatch_startup_owner_is_live(svc, &plan_id, &task_id, &owner.tab_id)
            });
            if let Some(owner) = begin_owner.as_ref().filter(|_| begin_owner_is_live) {
                Err(PlanError::DispatchInFlight {
                    attempt_id: owner.attempt_id.clone(),
                    tab_id: owner.tab_id.clone(),
                })
            } else {
                store.begin_task_dispatch_with_reserved_attempt_and_gates_and_observation(
                    &plan_id,
                    &task_id,
                    &reserved_tab_id,
                    &spawn_params
                        .title
                        .clone()
                        .unwrap_or_else(|| "Worker".into()),
                    starting_lane,
                    execution_intent,
                    allowance,
                    Some(&resolved_provider),
                    &reserved_attempt_id,
                    Some(authorized_gates.clone()),
                    placement,
                    dispatched_model,
                    selected_model.clone(),
                    dispatched_provider_resolved_model,
                    dispatched_reasoning_effort,
                    launch_profile,
                    begin_owner,
                    begin_owner_is_live,
                    true,
                    now,
                )
            }
        })
        .and_then(|started| {
            if let Some(cwd) = spawn_params.cwd.as_ref().filter(|_| {
                requires_repository
                    && spawn_params.execution_intent
                        == Some(codeterm_service_core::plan::ExecutionIntent::Mutating)
            }) {
                if let Err(error) = store.record_attempt_checkout(
                    &plan_id,
                    &task_id,
                    &reserved_tab_id,
                    &started.attempt_id,
                    cwd.clone(),
                    now,
                ) {
                    store.roll_back_task_dispatch_attempt(
                        &started.undo,
                        &reserved_tab_id,
                        &started.attempt_id,
                        now,
                    )?;
                    return Err(error);
                }
            }
            Ok(started)
        });
    let (started, provisioned_lane) =
        match finish_unreserved_dispatch(attempt_start, provisioned_lane, || {
            reservation_tab.close()
        }) {
            Ok(started) => started,
            Err(error) => {
                if error
                    .data
                    .as_ref()
                    .and_then(|data| data.get("type"))
                    .and_then(Value::as_str)
                    != Some("dispatch_in_flight")
                {
                    emit_plan_changed(svc, &plan_id);
                }
                return Err(error);
            }
        };
    let attempt_id = started.attempt_id;
    let rollback = started.undo;
    let provisioned_lane = match todo_seed::seed_acceptance_or_rollback(todo_seed::DispatchSeed {
        svc,
        store,
        plan_id: &plan_id,
        task_id: &task_id,
        parent_tab_id: &parent_tab_id,
        tab_id: &reserved_tab_id,
        attempt_id: &attempt_id,
        seed: &seed,
        rollback: &rollback,
        provisioned_lane,
        now,
    }) {
        Ok(lane) => lane,
        Err(error) => return Err(error),
    };
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let spawn = match spawn_agent(svc, spawn_params, Some(ready_tx)) {
        Ok(spawn) => spawn,
        Err(error) => {
            let cleanup = roll_back_dispatch_attempt(
                store,
                &rollback,
                &reserved_tab_id,
                &attempt_id,
                provisioned_lane,
                || reservation_tab.close(),
                now,
            );
            log::warn!(
                "[task-dispatch] dispatch_startup_failed plan={} task={} tab={} attempt={} stage=agent_spawn cause={error}",
                plan_id,
                task_id,
                reserved_tab_id,
                attempt_id
            );
            emit_plan_changed(svc, &plan_id);
            return Err(dispatch_failed(
                "agent_spawn",
                match cleanup {
                    Ok(()) => error,
                    Err(cleanup) => format!("{error}; {cleanup}"),
                },
            ));
        }
    };
    if spawn.tab_id != reserved_tab_id {
        let cause = format!(
            "spawn claimed Tab {}, expected reserved Tab {reserved_tab_id}",
            spawn.tab_id
        );
        let cleanup = roll_back_dispatch_attempt(
            store,
            &rollback,
            &reserved_tab_id,
            &attempt_id,
            provisioned_lane,
            || {
                let reserved_cleanup = reservation_tab.close();
                let spawned_cleanup = close_tab_if_present(svc, &spawn.tab_id);
                reserved_cleanup.and(spawned_cleanup)
            },
            now,
        );
        log::warn!(
            "[task-dispatch] dispatch_startup_failed plan={} task={} tab={} attempt={} stage=tab_reservation cause={cause}",
            plan_id,
            task_id,
            reserved_tab_id,
            attempt_id
        );
        emit_plan_changed(svc, &plan_id);
        return Err(dispatch_failed(
            "tab_reservation",
            match cleanup {
                Ok(()) => cause,
                Err(cleanup) => format!("{cause}; {cleanup}"),
            },
        ));
    }
    reservation_tab.mark_spawned();
    let tab_id = spawn.tab_id.clone();
    drop(dispatch_reservation);
    // The resolved effort is attempt telemetry; failing to record it must not kill a live Worker.
    if let Err(error) = svc.persist_dispatched_attempt_reasoning_effort(
        &plan_id,
        &task_id,
        &tab_id,
        &attempt_id,
        now,
    ) {
        log::warn!(
            "[task-dispatch] attempt_reasoning_effort_not_recorded plan={} task={} tab={} attempt={} cause={error}",
            plan_id,
            task_id,
            tab_id,
            attempt_id
        );
    }
    if let Err(error) =
        store.set_attempt_plan_skill_manifest(&plan_id, &task_id, &attempt_id, manifest, now)
    {
        let cleanup = roll_back_dispatch_attempt(
            store,
            &rollback,
            &tab_id,
            &attempt_id,
            provisioned_lane,
            || close_tab_if_present(svc, &tab_id),
            now,
        );
        log::warn!(
            "[task-dispatch] dispatch_startup_failed plan={} task={} tab={} attempt={} stage=skill_manifest cause={error}",
            plan_id,
            task_id,
            tab_id,
            attempt_id
        );
        emit_plan_changed(svc, &plan_id);
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
            &rollback,
            &tab_id,
            &attempt_id,
            provisioned_lane,
            || close_tab_if_present(svc, &tab_id),
            now,
        );
        log::warn!(
            "[task-dispatch] dispatch_startup_failed plan={} task={} tab={} attempt={} stage=task_binding cause={error}",
            plan_id,
            task_id,
            tab_id,
            attempt_id
        );
        emit_plan_changed(svc, &plan_id);
        return Err(dispatch_failed(
            "task_binding",
            match cleanup {
                Ok(()) => error,
                Err(cleanup) => format!("{error}; {cleanup}"),
            },
        ));
    }
    svc.persist_current_attempt_session_model(&tab_id);
    emit_plan_changed(svc, &plan_id);
    let startup = DispatchStartup {
        svc: svc.clone(),
        plan_id: plan_id.clone(),
        task_id: task_id.clone(),
        tab_id: tab_id.clone(),
        startup_attempt_id: spawn.pty_id.clone(),
        attempt_id: attempt_id.clone(),
        parent_tab_id,
        worker_title: spawn.title.clone(),
        rollback,
        lane_for_commit: lane_for_commit.clone(),
        canonical_project_path: canonical_cwd.clone(),
        provenance: initial_provenance.clone(),
        provisioned_lane,
        lane_protection,
        spawn_command: format!("domios plan task dispatch {plan_id} {task_id}"),
    };
    self::startup::spawn_startup(svc, startup, ready_rx);

    let model_fallback = spawn.model_fallback.clone();
    let mut result = json!({
        "ok": true,
        "plan_id": plan_id,
        "task_id": task_id,
        "attempt_id": attempt_id,
        "agent": resolved_provider,
        "authorized_gates": authorized_gates,
        "placement": placement,
        "placement_reason": placement_reason,
    });
    if let Some(provenance) = initial_provenance.as_ref() {
        result["lane"] = dispatch_response_lane(provenance, &canonical_cwd, &lane_for_commit);
    }
    apply_dispatch_starting_contract(&mut result, &tab_id, &spawn);
    if !warnings.is_empty() {
        result["warnings"] = json!(warnings);
    }
    if let Some(model_fallback) = model_fallback {
        result["model_fallback"] = json!(model_fallback);
    }
    if let Some(speed) = spawn_params_speed.as_deref() {
        store
            .update_task_speed(&plan_id, &task_id, Some(speed), now)
            .map_err(plan_rpc_err)?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
