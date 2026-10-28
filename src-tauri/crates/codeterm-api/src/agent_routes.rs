use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

use crate::routes::{check_auth, AppState};

pub(crate) use crate::agent_restart_routes::{agent_restart_handler, agent_restart_response};

#[cfg(test)]
use crate::agent_restart_routes::agent_restart_failure_response;

mod model_error;
mod mutation_authority;
mod role_mutations;
pub use role_mutations::{
    clear_tab_role, make_orchestrator, make_planner, set_can_spawn, set_tab_role,
};
mod pair_headers;
pub(crate) mod spawn_ownership;
mod spawn_rejection;
mod spawn_request;
mod worker_transfer;

pub(super) use pair_headers::create_orchestration_pair;
use spawn_ownership::ownership_for_spawning_peer;
pub(crate) use spawn_rejection::SpawnRejection;

const CONFIGURE_DELIVERY_DEADLINE: std::time::Duration = std::time::Duration::from_secs(90);

use codeterm_service_core::provider::values_list_agent_provider_values as list_agent_provider_values;

#[derive(Deserialize)]
pub struct SpawnAgentBody {
    /// Agent provider to run. Omitted resolves the configured General Agent.
    agent: Option<String>,
    /// Claude agent name from .claude/agents/ (e.g. "architect" → --agent architect)
    agent_name: Option<String>,
    /// Extra args for the agent
    args: Option<Vec<String>>,
    cwd: Option<String>,
    title: Option<String>,
    description: Option<String>,
    role: Option<String>,
    role_profile: Option<String>,
    parent_tab_id: Option<String>,
    #[serde(alias = "plan")]
    plan_id: Option<String>,
    can_spawn: Option<bool>,
    task: Option<String>,
    task_id: Option<String>,
    #[serde(alias = "plan-task")]
    plan_task: Option<String>,
    /// Gate tokens a task-less read-only Worker may claim as executed.
    #[serde(default)]
    authorized_gates: Vec<String>,
    execution_intent: Option<codeterm_service_core::plan::ExecutionIntent>,
    sandbox: Option<String>,
    provider_reserve_pct: Option<f32>,
    workspace_id: Option<String>,
    force_tab: Option<bool>,
    report_target_tab_id: Option<String>,
    /// User-facing report target (tab id or `main`/`general` alias). Collapses
    /// into `report_target_tab_id`; with `origin_peer` it's a tab on that peer.
    report_to: Option<String>,
    /// Originating peer, injected by the mesh relay from the authenticated signed
    /// token (never client-supplied). Present ⇒ this spawn was relayed across the
    /// mesh and the worker should report back to `report_to` on that peer.
    origin_peer: Option<String>,
    /// Tab id of the caller that ran `agent spawn` (CODETERM_TAB_ID).
    /// Recorded on the spawned tab for report routing fallback.
    spawned_by_tab_id: Option<String>,
    /// Team a roleless spawn attaches to; requires `creator_tab_id`, which the
    /// service authorizes as an existing member of that Team.
    team_id: Option<String>,
    creator_tab_id: Option<String>,
    /// Authenticated control ownership injected by the origin relay-out from its
    /// local caller tab; the target relay boundary re-stamps `peer_id`/`peer_name`.
    ownership: Option<codeterm_types::pty::RemoteOwnership>,
    /// Model selector — provider decides flag syntax via
    /// AgentProvider::model_launch_args.
    model: Option<String>,
    /// Reasoning-effort selector — capability-validated against the provider,
    /// rejected structurally if unsupported (no silent downgrade).
    #[serde(alias = "reasoning-effort")]
    reasoning_effort: Option<String>,
    speed: Option<String>,
    #[serde(alias = "system")]
    system_prompt: Option<String>,
    preset: Option<String>,
    execution_mode: Option<String>,
    charter: Option<String>,
    tick_interval_secs: Option<u64>,
    nudge_cooldown_secs: Option<u64>,
    max_actions_per_tick: Option<u32>,
    activate: Option<bool>,
    #[serde(default)]
    require_delivery_ack: bool,
    /// Audience for input-request notifications. `automation` declares a tab
    /// (bench/probe) whose prompts have no human owner and must never bell.
    #[serde(default)]
    audience: codeterm_service_core::multiplexer::types::TabAudience,
}

/// Orchestrator-side remote (cross-mesh) workers. `?tab=<id>` filters to one
/// orchestrator's workers; otherwise returns the full `{ tab_id: [...] }` map.
pub async fn remote_workers(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let reg = &state.svc.remote_workers;
    let value = match params.get("tab").filter(|s| !s.is_empty()) {
        Some(tab) => serde_json::json!(reg.for_orchestrator(tab)),
        None => serde_json::json!(reg.snapshot()),
    };
    Ok(Json(value))
}

#[derive(Deserialize)]
pub struct RecordRemoteWorkerBody {
    /// Local report-target / orchestrator tab the remote worker reports to
    /// (tab id or `main`/`general` alias). Resolved the same way as the
    /// inbound report path so spawn-time and report-time entries share a key.
    orchestrator_tab: String,
    /// Paired peer (instance name) the worker runs on.
    peer: String,
    /// The worker's tab id on that peer.
    remote_tab: String,
    title: Option<String>,
    model: Option<String>,
}

/// Register a freshly spawned cross-mesh worker on the origin before its first
/// report arrives, so the sidebar can ghost it under its orchestrator
/// immediately. Called by the CLI right after a successful `agent spawn --mesh`.
pub async fn record_remote_worker(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<RecordRemoteWorkerBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    if body.peer.trim().is_empty() || body.remote_tab.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "peer and remote_tab required".into(),
        ));
    }
    let orchestrator = crate::routes::resolve_tab_async(&state, &body.orchestrator_tab)
        .await
        .map_err(|(s, m)| (s, m.to_string()))?;
    let title = body
        .title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&body.remote_tab);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    state.svc.remote_workers.record_spawn(
        &orchestrator,
        &body.peer,
        &body.remote_tab,
        title,
        body.model.clone(),
        now_ms,
    );
    state.svc.emit_remote_workers_changed(&orchestrator);
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub struct ForgetRemoteWorkerBody {
    /// Foreign tab id on the remote peer.
    remote_tab: String,
    peer: Option<String>,
    orchestrator_tab: Option<String>,
}

/// Dismiss a remote-worker ghost from the origin sidebar (e.g. after the remote
/// tab was closed). Also reached implicitly via `DELETE /api/tabs/{id}/role`
/// when `{id}` is not a local tab.
pub async fn forget_remote_worker(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<ForgetRemoteWorkerBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    if body.remote_tab.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "remote_tab required".into()));
    }
    let orchestrator = match body.orchestrator_tab.as_deref().filter(|s| !s.is_empty()) {
        Some(tab) => Some(
            crate::routes::resolve_tab_async(&state, tab)
                .await
                .map_err(|(s, m)| (s, m.to_string()))?,
        ),
        None => None,
    };
    let touched = state.svc.remote_workers.forget_by_remote_tab(
        body.remote_tab.trim(),
        body.peer.as_deref().filter(|s| !s.is_empty()),
        orchestrator.as_deref(),
    );
    if touched.is_empty() {
        return Err((
            StatusCode::NOT_FOUND,
            format!("no remote worker matching {}", body.remote_tab.trim()),
        ));
    }
    for orch in &touched {
        state.svc.emit_remote_workers_changed(orch);
    }
    Ok(Json(
        serde_json::json!({"ok": true, "orchestrators": touched}),
    ))
}

pub async fn spawn_agent(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(mut body): Json<SpawnAgentBody>,
) -> Result<Json<serde_json::Value>, SpawnRejection> {
    check_auth(&state, &headers).map_err(|(s, m)| SpawnRejection {
        status: s,
        body: serde_json::json!({"ok": false, "error": {"code": "unauthorized", "message": m.to_string()}}),
    })?;
    codeterm_service_core::service::spawn_title_validate_agent_tab_title(
        body.title.as_deref(),
        body.agent_name.as_deref(),
    )
    .map_err(|message| SpawnRejection::new(StatusCode::BAD_REQUEST, "bad_request", message))?;
    if let Some(description) = body.description.as_deref() {
        codeterm_service_core::service::normalize_tab_description(description).map_err(
            |message| SpawnRejection::new(StatusCode::BAD_REQUEST, "bad_request", message),
        )?;
    }
    if crate::auth::caller_peer_id(&state, &headers).is_none() {
        if let Some(caller) = crate::auth::tab_caller(&state, &headers)
            .map_err(|(status, message)| SpawnRejection::new(status, "forbidden", message))?
        {
            for claim in [
                body.spawned_by_tab_id.as_deref(),
                body.creator_tab_id.as_deref(),
            ] {
                caller.validate_claim(claim).map_err(|message| {
                    SpawnRejection::new(StatusCode::FORBIDDEN, "forbidden", message)
                })?;
            }
            if body.origin_peer.is_some() {
                return Err(SpawnRejection::new(
                    StatusCode::FORBIDDEN,
                    "forbidden",
                    "Tab credentials cannot supply peer provenance".into(),
                ));
            }
            body.spawned_by_tab_id = Some(caller.tab_id.clone());
            body.creator_tab_id = Some(caller.tab_id);
        }
    }
    let require_delivery_ack = body.require_delivery_ack;
    let spawning_peer = crate::auth::caller_peer_id(&state, &headers).map(|peer_id| {
        let peer_name = state
            .mesh
            .lock()
            .paired_peers
            .get(&peer_id)
            .map(|peer| peer.instance_name.clone())
            .unwrap_or_default();
        (peer_id, peer_name)
    });
    let ownership = match spawning_peer {
        Some((peer_id, peer_name)) => Some(ownership_for_spawning_peer(
            body.ownership.take(),
            &peer_id,
            &peer_name,
        )),
        None => body.ownership.take(),
    };
    let role_probe = validate_spawn_role(body.role.clone(), body.role_profile.clone()).map_err(
        |(s, m)| SpawnRejection {
            status: s,
            body: serde_json::json!({"ok": false, "error": {"code": "bad_request", "message": m}}),
        },
    )?;
    let mut params = spawn_request::into_params(body, role_probe, ownership);
    state
        .svc
        .inherit_neighbor_agent_spawn(&mut params)
        .await
        .map_err(SpawnRejection::from)?;
    codeterm_service_core::service::execution_sandbox_enforce_delegation_ceiling(
        &state.svc,
        &params,
        crate::tab_routes::caller_tab_header(&headers).as_deref(),
    )
    .map_err(|message| SpawnRejection::new(StatusCode::FORBIDDEN, "sandbox_ceiling", message))?;
    let cwd_params = params.clone();
    spawn_rejection::validate_spawn_cwd(
        &state.svc,
        cwd_params.workspace_id.as_deref(),
        cwd_params
            .parent_tab_id
            .as_deref()
            .or(cwd_params.spawned_by_tab_id.as_deref()),
        cwd_params.cwd.as_deref(),
    )
    .await?;
    let default_provider = state.svc.configuration().general_agent.agent_provider;
    let provider_id = params.agent.as_deref().unwrap_or(&default_provider);
    model_error::validate_agent_model(
        &state.svc.agent_registry,
        provider_id,
        params.model.as_deref(),
    )?;
    let plan_task = params.plan_task.clone();
    let (ready_tx, ready_rx) = if require_delivery_ack {
        let (tx, rx) = tokio::sync::oneshot::channel();
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };
    let result = match state.svc.spawn_agent(params, ready_tx).await {
        Ok(result) => result,
        Err(error) => {
            spawn_rejection::validate_spawn_cwd(
                &state.svc,
                cwd_params.workspace_id.as_deref(),
                cwd_params
                    .parent_tab_id
                    .as_deref()
                    .or(cwd_params.spawned_by_tab_id.as_deref()),
                cwd_params.cwd.as_deref(),
            )
            .await?;
            return Err(SpawnRejection::from(error));
        }
    };
    let launch = match ready_rx {
        Some(ready_rx) => {
            verify_spawn_delivery(&state, &result.tab_id, ready_rx).await?;
            None
        }
        None => match spawn_rejection::await_spawn_launch(&state.svc, &result.tab_id).await {
            Ok(launch) => Some(launch),
            Err(error) => {
                spawn_rejection::validate_spawn_cwd(
                    &state.svc,
                    cwd_params.workspace_id.as_deref(),
                    cwd_params
                        .parent_tab_id
                        .as_deref()
                        .or(cwd_params.spawned_by_tab_id.as_deref()),
                    cwd_params.cwd.as_deref(),
                )
                .await?;
                return Err(error);
            }
        },
    };
    tokio::task::spawn_blocking(move || {
        let ack_warning = if let Some(task_ref) = plan_task.as_deref() {
            match state.svc.arm_bound_worker_ack_deadline_for_tab(
                &result.tab_id,
                format!("domios agent spawn --plan-task {task_ref}"),
            ) {
                Ok(true) => None,
                Ok(false) => Some(format!(
                    "worker ACK tracking was not armed for task-bound tab {}",
                    result.tab_id
                )),
                Err(error) => Some(format!(
                    "worker ACK tracking failed for tab {}: {error}",
                    result.tab_id
                )),
            }
        } else {
            None
        };

        let mut resp = serde_json::json!({
            "id": result.tab_id,
            "tab_id": result.tab_id,
            "team_id": result.team_id,
            "pty_id": result.pty_id,
            "agent": result.agent,
            "model": result.model,
            "planned_model": result.planned_model,
            "requested_model": result.requested_model,
            "provider_resolved_model": result.provider_resolved_model,
            "actual_model": result.actual_model,
            "reasoning_effort": result.reasoning_effort,
            "reasoning_effort_provenance": result.reasoning_effort_provenance,
            "title": result.title,
            "description": result.description,
            "role": result.role,
            "role_profile": result.role_profile,
        });
        if require_delivery_ack {
            resp["delivery_state"] = serde_json::json!("confirmed");
        }
        let warning = match launch.as_ref() {
            Some(launch) => {
                resp["launch_state"] = serde_json::json!(launch.launch_state());
                launch.merged_warning(&result.tab_id, result.warning.as_deref())
            }
            None => result.warning.clone(),
        };
        if let Some(warning) = warning {
            resp["warning"] = serde_json::json!(warning);
        }
        if let Some(warning) = ack_warning {
            resp["orchestration_incident"] = serde_json::json!({
                "kind": "worker_ack_tracking_failed",
                "impact": warning,
                "fallback": "Inspect the existing tab once. Do not blind-resend.",
            });
        }
        if let Some(ref fallback) = result.model_fallback {
            resp["model_fallback"] = serde_json::json!(fallback);
        }
        if let Some(ref notice) = result.budget_notice {
            resp["budget_notice"] = serde_json::to_value(notice).unwrap_or_default();
        }
        Ok(Json(resp))
    })
    .await
    .map_err(|_| {
        SpawnRejection::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "spawn_response_failed",
            "blocking task failed".to_string(),
        )
    })?
}

/// A launch that did not acknowledge, or left its first turn pending, is rolled
/// back so the caller never keeps a tab that silently holds an undelivered task.
pub(crate) async fn verify_spawn_delivery(
    state: &AppState,
    tab_id: &str,
    ready_rx: tokio::sync::oneshot::Receiver<Result<(), String>>,
) -> Result<(), SpawnRejection> {
    let ready = await_spawn_delivery(ready_rx).await;
    let owner = state.clone();
    let target = tab_id.to_owned();
    let delivery = tokio::task::spawn_blocking(move || {
        let state = owner;
        let tab_id = target.as_str();
        let delivery = ready.and_then(|()| {
            let mux = state.svc.mux.read();
            let tab = mux
                .get_tab(tab_id)
                .ok_or_else(|| "agent launch acknowledged after its Tab disappeared".to_string())?;
            require_no_pending_delivery(
                tab.pending_delivery
                    .as_ref()
                    .map(|pending| format!("{:?}", pending.state())),
            )
        });
        delivery
    })
    .await
    .map_err(|error| {
        SpawnRejection::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "delivery_failed",
            error.to_string(),
        )
    })?;
    if let Err(message) = delivery {
        let _ = state
            .svc
            .close_tab_with_reason_async(
                tab_id,
                codeterm_service_core::multiplexer::types::TabCloseReason::SpawnRollback,
            )
            .await;
        return Err(SpawnRejection::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "delivery_failed",
            message,
        ));
    }
    Ok(())
}

fn require_no_pending_delivery(state: Option<String>) -> Result<(), String> {
    match state {
        None => Ok(()),
        Some(state) => Err(format!(
            "agent launch ended without confirmed task delivery; pending state: {state}"
        )),
    }
}

async fn await_spawn_delivery(
    ready_rx: tokio::sync::oneshot::Receiver<Result<(), String>>,
) -> Result<(), String> {
    match tokio::time::timeout(CONFIGURE_DELIVERY_DEADLINE, ready_rx).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err("agent launch ended without a delivery acknowledgement".into()),
        Err(_) => Err(format!(
            "agent launch did not acknowledge task delivery within {:?}",
            CONFIGURE_DELIVERY_DEADLINE
        )),
    }
}

fn validate_spawn_role(
    role: Option<String>,
    role_profile: Option<String>,
) -> Result<codeterm_service_core::service::SpawnAgentParams, (StatusCode, String)> {
    let mut role_probe = codeterm_service_core::service::SpawnAgentParams {
        role,
        role_profile,
        ..Default::default()
    };
    role_probe
        .normalize_role_profile()
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    if let Some(ref role) = role_probe.role {
        codeterm_service_core::service::ServiceContext::parse_role(role)
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    }
    Ok(role_probe)
}

#[derive(Deserialize)]
pub struct BatchBody {
    commands: Vec<BatchOp>,
}

#[derive(Deserialize)]
pub(crate) struct BatchOp {
    method: String,
    #[serde(default)]
    params: serde_json::Value,
    /// Optional ID — other commands can reference this result via "$id"
    id: Option<String>,
}

pub async fn batch_commands(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<BatchBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    if crate::auth::tab_caller(&state, &headers)?.is_some() {
        return Err((
            StatusCode::FORBIDDEN,
            "Agent batch execution requires the host credential".into(),
        ));
    }
    let ops: Vec<codeterm_service_core::service::BatchOp> = body
        .commands
        .into_iter()
        .map(|c| codeterm_service_core::service::BatchOp {
            method: c.method,
            params: c.params,
            id: c.id,
        })
        .collect();
    let results = codeterm_orchestration::batch(&state.svc, ops).await;
    Ok(Json(serde_json::json!({"results": results})))
}

pub async fn get_tab_status(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let tab_id = crate::routes::resolve_tab_async(&state, &id)
        .await
        .map_err(|(s, m)| (s, m.to_string()))?;
    tokio::task::spawn_blocking(move || {
        let info = state
            .svc
            .get_tab_status(&tab_id)
            .map_err(|e| (StatusCode::NOT_FOUND, e))?;
        Ok(Json(serde_json::to_value(info).unwrap()))
    })
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "blocking task failed".to_string(),
        )
    })?
}

#[derive(Deserialize)]
pub struct ReportBody {
    message: String,
    plan_id: Option<String>,
    task_id: Option<String>,
    kind: Option<String>,
    report_outcome: Option<codeterm_types::agent::ReportOutcomeClass>,
    #[serde(flatten)]
    details: codeterm_service_core::agent::reports::ReportDetails,
}

pub async fn submit_report(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(mut body): Json<ReportBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || {
        check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
        let id = state
            .svc
            .mux
            .read()
            .resolve_tab_id(&id)
            .ok_or((StatusCode::NOT_FOUND, "Reporting tab not found".into()))?;
        let mut details = Some(
            serde_json::to_value(body.details)
                .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?,
        );
        details
            .as_mut()
            .ok_or((StatusCode::BAD_REQUEST, "Missing report details".into()))?["tab_id"] =
            serde_json::json!(id);
        let credential = headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .unwrap_or_default();
        state
            .svc
            .bind_local_rpc(credential, "agent.report", &mut details)
            .map_err(|error| (StatusCode::FORBIDDEN, error))?;
        body.details = serde_json::from_value(details.unwrap_or_default())
            .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
        let result = state
            .svc
            .submit_report_with_source_outcome(
                &id,
                &body.message,
                body.details,
                body.plan_id.as_deref(),
                body.task_id.as_deref(),
                body.kind.as_deref(),
                body.report_outcome,
            )
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
        let mut response = serde_json::json!({
            "ok": true,
            "accepted": true,
            "state": "recorded",
            "report_id": result.report_id,
            "target_tab_id": result.orchestrator_tab_id,
            "report_outcome": result.report_outcome,
        });
        if let Some((plan_id, task_id)) = result.persisted_task {
            response["task_report"] = serde_json::json!({
                "persisted": true,
                "plan_id": plan_id,
                "task_id": task_id,
                "report_outcome": result.report_outcome,
            });
        }
        if let Some(rejection) = result.rejected_task_transition {
            response["task_report"] = serde_json::json!({
                "persisted": false,
                "plan_id": rejection.plan_id,
                "task_id": rejection.task_id,
                "transition_rejected": true,
                "cause": rejection.cause,
                "report_outcome": rejection.report_outcome,
            });
        }
        if let Some((plan_id, stride_id)) = result.persisted_plan_evidence {
            response["plan_report"] = serde_json::json!({
                "persisted": true,
                "plan_id": plan_id,
                "stride_id": stride_id,
                "task_id": serde_json::Value::Null,
                "task_transition": false,
            });
        }
        Ok(Json(response))
    })
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "blocking task failed".to_string(),
        )
    })?
}

#[derive(Deserialize)]
pub struct ReportsQuery {
    clear: Option<bool>,
}

pub async fn get_reports(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<ReportsQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let clear = query.clear.unwrap_or(false);
    let reports = state.svc.report_store.lock().get(&id, clear);
    Ok(Json(serde_json::to_value(&reports).unwrap_or_default()))
}

pub async fn respond_prompt(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Response, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let tab_id = crate::routes::resolve_tab_async(&state, &raw_tab_id)
        .await
        .map_err(|(s, m)| (s, m.to_string()))?;
    let value = body
        .get("value")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Missing 'value' field".to_string()))?
        .to_string();
    let identity = body
        .get("identity")
        .map(|identity| {
            serde_json::from_value::<codeterm_types::pty::PromptIdentity>(identity.clone())
        })
        .transpose()
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
    let svc = state.svc.clone();
    let pid = tab_id.clone();
    let outcome = tokio::task::spawn_blocking(move || match identity {
        Some(identity) => svc.respond_prompt_with_identity(&pid, &value, &identity),
        None => svc.respond_prompt(&pid, &value),
    })
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "respond_prompt task join failed".to_string(),
        )
    })?
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(respond_prompt_response(outcome))
}

pub(crate) fn respond_prompt_response(
    outcome: codeterm_service_core::service::tab::RespondPromptOutcome,
) -> Response {
    let (status, body) = match outcome {
        codeterm_service_core::service::tab::RespondPromptOutcome::Sent
        | codeterm_service_core::service::tab::RespondPromptOutcome::Structured => {
            (StatusCode::OK, serde_json::json!({"ok": true}))
        }
        codeterm_service_core::service::tab::RespondPromptOutcome::StalePrompt => (
            StatusCode::CONFLICT,
            serde_json::json!({"ok": false, "reason": "stale_prompt"}),
        ),
        codeterm_service_core::service::tab::RespondPromptOutcome::PromptNotOnScreen => (
            StatusCode::CONFLICT,
            serde_json::json!({"ok": false, "reason": "prompt_not_on_screen"}),
        ),
        codeterm_service_core::service::tab::RespondPromptOutcome::NoPty => (
            StatusCode::CONFLICT,
            serde_json::json!({"ok": false, "reason": "no_pty"}),
        ),
        codeterm_service_core::service::tab::RespondPromptOutcome::InvalidOption => (
            StatusCode::BAD_REQUEST,
            serde_json::json!({"ok": false, "reason": "invalid_option"}),
        ),
    };
    (status, Json(body)).into_response()
}

mod identity;
pub use identity::{clear_prompt, set_agent_type};

/// List all registered agent providers as `ProviderDescriptor`s.
pub async fn list_agent_providers(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    list_agent_providers_blocking(state.svc.provider_registry.clone()).await
}

async fn list_agent_providers_blocking(
    registry: std::sync::Arc<codeterm_service_core::provider::ProviderRegistry>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let started = std::time::Instant::now();
    tokio::task::spawn_blocking(move || {
        let queue_us = started.elapsed().as_micros();
        let result = Json(serde_json::Value::Array(list_agent_provider_values(
            &registry,
        )));
        log::debug!(
            "[providers.route] queue_us={} total_us={}",
            queue_us,
            started.elapsed().as_micros()
        );
        result
    })
    .await
    .map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("provider catalog worker failed: {error}"),
        )
    })
}

/// Web-mirror twin of the `restart_general_agent` Tauri command — kills the
/// protected General Agent tab's PTY and relaunches it. Fire-and-forget,
/// same as the desktop command.
pub async fn restart_general_agent(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let svc = state.svc.clone();
    let mux = svc.mux.clone();
    let pty = svc.pty.clone();
    let screen_state = svc.screen_state.clone();
    let config = state.config.clone();
    std::thread::spawn(move || {
        codeterm_service_core::service::general_agent::restart_general_agent(
            &svc,
            &mux,
            &pty,
            &screen_state,
            &config,
        );
    });
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn restart_general_agent_tab(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    axum::extract::Path(tab_id): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let svc = state.svc.clone();
    let mux = svc.mux.clone();
    let should_restart =
        codeterm_service_core::service::general_agent::claim_general_agent_tab_retry(&tab_id, &mux)
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    if !should_restart {
        return Ok(Json(serde_json::json!({"ok": true, "restarted": false})));
    }
    let pty = svc.pty.clone();
    let screen_state = svc.screen_state.clone();
    let config = state.config.clone();
    std::thread::spawn(move || {
        codeterm_service_core::service::general_agent::restart_general_agent(
            &svc,
            &mux,
            &pty,
            &screen_state,
            &config,
        );
    });
    Ok(Json(serde_json::json!({"ok": true, "restarted": true})))
}

#[derive(serde::Deserialize)]
pub struct CompleteFirstRunBody {
    pub provider_id: String,
    pub full_access: bool,
}

/// Web-mirror twin of `complete_general_agent_first_run`.
pub async fn complete_general_agent_first_run(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<CompleteFirstRunBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    codeterm_service_core::service::general_agent::complete_first_run_core(
        &state.svc,
        &state.config,
        &body.provider_id,
        body.full_access,
    )
    .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// Web-mirror twin of `install_provider_in_tab`.
#[derive(Default, serde::Deserialize)]
pub struct InstallProviderQuery {
    #[cfg(debug_assertions)]
    fixture_dispatch_failure: Option<String>,
}

pub async fn install_provider_in_tab(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(provider_id): Path<String>,
    Query(query): Query<InstallProviderQuery>,
) -> Result<Json<codeterm_commands::commands::InstallTabResult>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let svc = state.svc.clone();
    tokio::task::spawn_blocking(move || install_provider_request(&svc, provider_id, query))
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("installer launch worker failed: {error}"),
            )
        })?
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

#[cfg(debug_assertions)]
fn install_provider_request(
    svc: &codeterm_service_core::service::ServiceState,
    provider_id: String,
    query: InstallProviderQuery,
) -> Result<codeterm_commands::commands::InstallTabResult, String> {
    match query
        .fixture_dispatch_failure
        .filter(|value| !value.is_empty())
    {
        Some(failure) => codeterm_commands::commands::install_provider_in_tab_with_fixture_failure(
            svc,
            provider_id,
            failure,
        ),
        None => codeterm_commands::commands::install_provider_in_tab_core(svc, provider_id),
    }
}

#[cfg(not(debug_assertions))]
fn install_provider_request(
    svc: &codeterm_service_core::service::ServiceState,
    provider_id: String,
    _query: InstallProviderQuery,
) -> Result<codeterm_commands::commands::InstallTabResult, String> {
    codeterm_commands::commands::install_provider_in_tab_core(svc, provider_id)
}

#[cfg(test)]
mod restart_tests;

#[cfg(test)]
mod tests;
