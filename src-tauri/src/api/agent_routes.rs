use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, resolve_pane, AppState};
use crate::multiplexer::types::AgentRole;

/// JSON error surface for `/api/agents/spawn`, so the mesh relay can parse it
/// instead of collapsing a bare `(StatusCode, String)` body to "HTTP 500".
pub(crate) struct SpawnRejection {
    status: StatusCode,
    body: serde_json::Value,
}

impl IntoResponse for SpawnRejection {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

impl From<String> for SpawnRejection {
    fn from(message: String) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            body: serde_json::json!({
                "ok": false,
                "error": {
                    "code": "spawn_failed",
                    "message": message,
                    "retry_hint": "transient — retry once; if it persists, fix the spawn request (--role/--plan-task/--cwd) or check the target's health with `codeterm mesh peers`",
                },
            }),
        }
    }
}

pub(crate) fn list_agent_provider_values(
    registry: &crate::provider::ProviderRegistry,
) -> Vec<serde_json::Value> {
    registry
        .describe_all()
        .into_iter()
        .filter_map(|descriptor| serde_json::to_value(descriptor).ok())
        .collect()
}

#[derive(Deserialize)]
pub(crate) struct SpawnAgentBody {
    /// Agent command to run (e.g. "claude", "aider", "goose"). Default: "claude"
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
    parent_pane_id: Option<String>,
    #[serde(alias = "plan")]
    plan_id: Option<String>,
    can_spawn: Option<bool>,
    task: Option<String>,
    task_id: Option<String>,
    #[serde(alias = "plan-task")]
    plan_task: Option<String>,
    workspace_id: Option<String>,
    force_tab: Option<bool>,
    report_target_pane_id: Option<String>,
    /// User-facing report target (pane id or `main`/`general` alias). Collapses
    /// into `report_target_pane_id`; with `origin_peer` it's a pane on that peer.
    report_to: Option<String>,
    /// Originating peer, injected by the mesh relay from the authenticated signed
    /// token (never client-supplied). Present ⇒ this spawn was relayed across the
    /// mesh and the worker should report back to `report_to` on that peer.
    origin_peer: Option<String>,
    skip_permissions: Option<bool>,
    /// Pane id of the caller that ran `agent spawn` (CODETERM_PANE_ID).
    /// Recorded on the spawned pane for report routing fallback.
    spawned_by_pane_id: Option<String>,
    /// Model selector — provider decides flag syntax via
    /// AgentProvider::model_launch_args.
    model: Option<String>,
    /// Reasoning-effort selector — capability-validated against the provider,
    /// rejected structurally if unsupported (no silent downgrade).
    #[serde(alias = "reasoning-effort")]
    reasoning_effort: Option<String>,
    #[serde(alias = "system")]
    system_prompt: Option<String>,
    preset: Option<String>,
    charter: Option<String>,
    tick_interval_secs: Option<u64>,
    nudge_cooldown_secs: Option<u64>,
    max_actions_per_tick: Option<u32>,
    activate: Option<bool>,
}

/// Orchestrator-side remote (cross-mesh) workers. `?pane=<id>` filters to one
/// orchestrator's workers; otherwise returns the full `{ pane_id: [...] }` map.
pub async fn remote_workers(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let reg = &state.svc.remote_workers;
    let value = match params.get("pane").filter(|s| !s.is_empty()) {
        Some(pane) => serde_json::json!(reg.for_orchestrator(pane)),
        None => serde_json::json!(reg.snapshot()),
    };
    Ok(Json(value))
}

#[derive(Deserialize)]
pub(crate) struct RecordRemoteWorkerBody {
    /// Local report-target / orchestrator pane the remote worker reports to
    /// (pane id or `main`/`general` alias). Resolved the same way as the
    /// inbound report path so spawn-time and report-time entries share a key.
    orchestrator_pane: String,
    /// Paired peer (instance name) the worker runs on.
    peer: String,
    /// The worker's pane id on that peer.
    remote_pane: String,
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
    if body.peer.trim().is_empty() || body.remote_pane.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "peer and remote_pane required".into(),
        ));
    }
    let orchestrator =
        resolve_pane(&state, &body.orchestrator_pane).map_err(|(s, m)| (s, m.to_string()))?;
    let title = body
        .title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&body.remote_pane);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    state.svc.remote_workers.record_spawn(
        &orchestrator,
        &body.peer,
        &body.remote_pane,
        title,
        body.model.clone(),
        now_ms,
    );
    state.svc.emit_remote_workers_changed(&orchestrator);
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub(crate) struct ForgetRemoteWorkerBody {
    /// Foreign pane id on the remote peer.
    remote_pane: String,
    peer: Option<String>,
    orchestrator_pane: Option<String>,
}

/// Dismiss a remote-worker ghost from the origin sidebar (e.g. after the remote
/// pane was closed). Also reached implicitly via `DELETE /api/panes/{id}/role`
/// when `{id}` is not a local pane.
pub async fn forget_remote_worker(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<ForgetRemoteWorkerBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    if body.remote_pane.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "remote_pane required".into()));
    }
    let orchestrator = body
        .orchestrator_pane
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|p| resolve_pane(&state, p).map_err(|(s, m)| (s, m.to_string())))
        .transpose()?;
    let touched = state.svc.remote_workers.forget_by_remote_pane(
        body.remote_pane.trim(),
        body.peer.as_deref().filter(|s| !s.is_empty()),
        orchestrator.as_deref(),
    );
    if touched.is_empty() {
        return Err((
            StatusCode::NOT_FOUND,
            format!("no remote worker matching {}", body.remote_pane.trim()),
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
    Json(body): Json<SpawnAgentBody>,
) -> Result<Json<serde_json::Value>, SpawnRejection> {
    check_auth(&state, &headers).map_err(|(s, m)| SpawnRejection {
        status: s,
        body: serde_json::json!({"ok": false, "error": {"code": "unauthorized", "message": m.to_string()}}),
    })?;
    let role_probe = validate_spawn_role(body.role.clone(), body.role_profile.clone()).map_err(
        |(s, m)| SpawnRejection {
            status: s,
            body: serde_json::json!({"ok": false, "error": {"code": "bad_request", "message": m}}),
        },
    )?;
    let params = crate::service::SpawnAgentParams {
        agent: body.agent,
        agent_name: body.agent_name,
        args: body.args,
        cwd: body.cwd,
        provisioned_cwd: None,
        title: body.title,
        title_provenance: Some(crate::service::SpawnIdentityProvenance::UserSupplied),
        description: body.description,
        role: role_probe.role,
        role_profile: role_probe.role_profile,
        parent_pane_id: body.parent_pane_id,
        plan_id: body.plan_id,
        can_spawn: body.can_spawn,
        task: body.task,
        starter_prompt: Default::default(),
        task_id: body.task_id,
        plan_task: body.plan_task,
        workspace_id: body.workspace_id,
        force_tab: body.force_tab,
        existing_tab_id: None,
        skip_orchestrator_onboarding: None,
        report_target_pane_id: body.report_to.or(body.report_target_pane_id),
        report_target_mesh_peer: body.origin_peer,
        skip_permissions: body.skip_permissions,
        spawned_by_pane_id: body.spawned_by_pane_id,
        model: body.model,
        planned_model: None,
        reasoning_effort: body.reasoning_effort,
        system_prompt: body.system_prompt,
        preset: body.preset,
        resume_session_id: None,
        charter: body.charter,
        tick_interval_secs: body.tick_interval_secs,
        nudge_cooldown_secs: body.nudge_cooldown_secs,
        max_actions_per_tick: body.max_actions_per_tick,
        activate: body.activate,
        manager_bootstrap: None,
        structured_prompt_server: None,
    };
    let result = state
        .svc
        .spawn_agent(params, None)
        .await
        .map_err(SpawnRejection::from)?;

    let mut resp = serde_json::json!({
        "id": result.pane_id,
        "pane_id": result.pane_id,
        "tab_id": result.tab_id,
        "pty_id": result.pty_id,
        "agent": result.agent,
        "model": result.model,
        "planned_model": result.planned_model,
        "requested_model": result.requested_model,
        "provider_resolved_model": result.provider_resolved_model,
        "actual_model": result.actual_model,
        "title": result.title,
        "description": result.description,
        "role": result.role,
        "role_profile": result.role_profile,
    });
    if let Some(ref warning) = result.warning {
        resp["warning"] = serde_json::json!(warning);
    }
    if let Some(ref fallback) = result.model_fallback {
        resp["model_fallback"] = serde_json::json!(fallback);
    }
    if let Some(ref notice) = result.budget_notice {
        resp["budget_notice"] = serde_json::to_value(notice).unwrap_or_default();
    }
    Ok(Json(resp))
}

fn validate_spawn_role(
    role: Option<String>,
    role_profile: Option<String>,
) -> Result<crate::service::SpawnAgentParams, (StatusCode, String)> {
    let mut role_probe = crate::service::SpawnAgentParams {
        role,
        role_profile,
        ..Default::default()
    };
    role_probe
        .normalize_role_profile()
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    if let Some(ref role) = role_probe.role {
        crate::service::ServiceContext::parse_role(role)
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    }
    Ok(role_probe)
}

/// Paired creation (task_7e5f9354): create an Orchestrator + Planner in one
/// action. The web-mirror twin of `codeterm orchestration new` — both call
/// `ServiceContext::create_orchestration_pair`.
pub async fn create_orchestration_pair(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<crate::service::OrchestrationPairParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let result = state
        .svc
        .create_orchestration_pair(body)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(result).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?))
}

#[derive(Deserialize)]
pub(crate) struct BatchBody {
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
    let ops: Vec<crate::service::BatchOp> = body
        .commands
        .into_iter()
        .map(|c| crate::service::BatchOp {
            method: c.method,
            params: c.params,
            id: c.id,
        })
        .collect();
    let results = state.svc.batch(ops).await;
    Ok(Json(serde_json::json!({"results": results})))
}

#[derive(Deserialize)]
pub(crate) struct SetRoleBody {
    role: String,
    #[serde(default)]
    role_profile: Option<String>,
    #[serde(default)]
    plan_id: Option<String>,
    parent_pane_id: Option<String>,
    can_spawn: Option<bool>,
}

pub async fn set_pane_role(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<SetRoleBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let mut role_probe = crate::service::SpawnAgentParams {
        role: Some(body.role),
        role_profile: body.role_profile,
        ..Default::default()
    };
    role_probe
        .normalize_role_profile()
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let role = role_probe.role.unwrap_or_else(|| "Worker".into());
    let parsed_role = crate::service::ServiceContext::parse_role(&role)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let was_orchestrator = {
        let mux = state.svc.mux.read();
        mux.get_pane(&id)
            .map(|p| p.role == Some(AgentRole::Orchestrator))
            .unwrap_or(false)
    };
    let destination = match parsed_role {
        AgentRole::Orchestrator => crate::service::AgentPlacement::Orchestrator {
            can_spawn: body.can_spawn.unwrap_or(true),
        },
        AgentRole::Worker => crate::service::AgentPlacement::Worker {
            parent_pane_id: body.parent_pane_id.ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    "Worker requires a parent_pane_id".to_string(),
                )
            })?,
        },
        AgentRole::Manager if role_probe.role_profile.as_deref() == Some("planner") => {
            crate::service::AgentPlacement::Planner {
                plan_id: body.plan_id.ok_or_else(|| {
                    (
                        StatusCode::BAD_REQUEST,
                        "Planner requires a plan_id".to_string(),
                    )
                })?,
            }
        }
        AgentRole::Manager => crate::service::AgentPlacement::Manager {
            parent_pane_id: body.parent_pane_id.ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    "Manager requires a parent_pane_id".to_string(),
                )
            })?,
            role_profile: role_probe.role_profile,
        },
    };
    state
        .svc
        .transition_agent(crate::service::AgentTransitionRequest {
            pane_id: id.clone(),
            destination,
            require_verified_session: true,
            planner_replacement: None,
        })
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    if parsed_role == AgentRole::Orchestrator {
        state
            .svc
            .maybe_deliver_orchestrator_promotion_onboarding(&id, was_orchestrator);
    }
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn clear_pane_role(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    state
        .svc
        .clear_pane_role(&id)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn make_orchestrator(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    state
        .svc
        .make_orchestrator(&id)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub(crate) struct MakePlannerBody {
    plan_id: String,
}

pub async fn make_planner(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<MakePlannerBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    state
        .svc
        .transition_plan_planner(
            &body.plan_id,
            Some(crate::plan::PlannerRef::Pane {
                pane_id: id,
                session_id: None,
                title: None,
            }),
        )
        .map_err(|error| (StatusCode::CONFLICT, error))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn get_pane_status(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let pane_id = resolve_pane(&state, &id).map_err(|(s, m)| (s, m.to_string()))?;
    let info = state
        .svc
        .get_pane_status(&pane_id)
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    Ok(Json(serde_json::to_value(info).unwrap()))
}

#[derive(Deserialize)]
pub(crate) struct ReportBody {
    message: String,
    plan_id: Option<String>,
    task_id: Option<String>,
    kind: Option<String>,
    #[serde(flatten)]
    details: crate::agent::reports::ReportDetails,
}

pub async fn submit_report(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ReportBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let result = state
        .svc
        .submit_report(
            &id,
            &body.message,
            body.details,
            body.plan_id.as_deref(),
            body.task_id.as_deref(),
            body.kind.as_deref(),
        )
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "accepted": true,
        "state": "recorded",
        "report_id": result.report_id,
        "target_tab_id": result.orchestrator_pane_id,
    })))
}

#[derive(Deserialize)]
pub(crate) struct ReportsQuery {
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

#[derive(Deserialize)]
pub(crate) struct CanSpawnBody {
    can_spawn: bool,
}

pub async fn set_can_spawn(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<CanSpawnBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    state
        .svc
        .set_can_spawn(&id, body.can_spawn)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub(crate) struct AgentRestartBody {
    #[serde(default)]
    force: bool,
    #[serde(default = "default_grace_secs")]
    grace_secs: u64,
}

fn default_grace_secs() -> u64 {
    crate::service::agent_restart::DEFAULT_GRACE_SECS
}

pub async fn agent_restart_handler(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
    body: Option<Json<AgentRestartBody>>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if let Err((status, msg)) = check_auth(&state, &headers) {
        return (status, msg.to_string()).into_response();
    }
    let pane_id = match resolve_pane(&state, &raw_pane_id) {
        Ok(p) => p,
        Err((status, msg)) => return (status, msg.to_string()).into_response(),
    };
    let body = body.map(|Json(b)| b).unwrap_or(AgentRestartBody {
        force: false,
        grace_secs: default_grace_secs(),
    });

    match state
        .svc
        .agent_restart(&pane_id, body.force, body.grace_secs)
    {
        Ok(outcome) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "restarting": true,
                "pane_id": outcome.pane_id,
                "provider": outcome.provider,
                "session_id": outcome.session_id,
                "session_was_stale": outcome.session_was_stale,
                "session_action": outcome.session_action(),
                "auto_approve": outcome.auto_approve,
                "permission_profile": outcome.permission_profile(),
                "force": outcome.force,
            })),
        )
            .into_response(),
        Err(crate::service::agent_restart::AgentRestartError::NotFound(m)) => {
            (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": m}))).into_response()
        }
        Err(crate::service::agent_restart::AgentRestartError::BadRequest(m)) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": m})),
        )
            .into_response(),
        Err(crate::service::agent_restart::AgentRestartError::RateLimited { retry_after_secs }) => {
            let mut h = HeaderMap::new();
            h.insert("Retry-After", retry_after_secs.to_string().parse().unwrap());
            (
                StatusCode::TOO_MANY_REQUESTS,
                h,
                Json(serde_json::json!({
                    "error": "rate limited",
                    "retry_after_secs": retry_after_secs,
                })),
            )
                .into_response()
        }
        Err(crate::service::agent_restart::AgentRestartError::Internal(m)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": m})),
        )
            .into_response(),
    }
}

pub async fn respond_prompt(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let pane_id = resolve_pane(&state, &raw_pane_id).map_err(|(s, m)| (s, m.to_string()))?;
    let value = body
        .get("value")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Missing 'value' field".to_string()))?
        .to_string();
    let svc = state.svc.clone();
    let pid = pane_id.clone();
    tokio::task::spawn_blocking(move || svc.respond_prompt(&pid, &value))
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "respond_prompt task join failed".to_string(),
            )
        })?
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// Clear pending_prompt state without sending to PTY (safe for cleanup/reset).
pub async fn clear_prompt(
    AxumState(state): AxumState<AppState>,
    Path(raw_pane_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let pane_id = resolve_pane(&state, &raw_pane_id).map_err(|(s, m)| (s, m.to_string()))?;
    state
        .svc
        .clear_prompt(&pane_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// Set agent type on a pane (enables chat view for that agent's provider).
pub async fn set_agent_type(
    AxumState(state): AxumState<AppState>,
    Path(pane_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let agent_type = body
        .get("agent_type")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    state.svc.set_pane_agent_type(&pane_id, agent_type);
    state.svc.emit_state_changed();
    Ok(Json(serde_json::json!({"ok": true})))
}

/// List all registered agent providers as `ProviderDescriptor`s.
pub async fn list_agent_providers(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    Ok(Json(serde_json::Value::Array(list_agent_provider_values(
        &state.svc.provider_registry,
    ))))
}

/// Public provider list for local plugin verification/UI smoke checks.
/// This exposes provider descriptors only; mutating agent routes remain
/// authenticated.
pub async fn list_agent_providers_public(
    AxumState(state): AxumState<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    Ok(Json(serde_json::Value::Array(list_agent_provider_values(
        &state.svc.provider_registry,
    ))))
}

/// Web-mirror twin of the `restart_general_agent` Tauri command — kills the
/// protected General Agent pane's PTY and relaunches it. Fire-and-forget,
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
    let app = svc.app_handle.clone();
    std::thread::spawn(move || {
        crate::service::general_agent::restart_general_agent(
            &svc,
            &mux,
            &pty,
            &screen_state,
            &config,
            &app,
        );
    });
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn restart_general_agent_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    axum::extract::Path(pane_id): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let svc = state.svc.clone();
    let mux = svc.mux.clone();
    let should_restart =
        crate::service::general_agent::claim_general_agent_pane_retry(&pane_id, &mux)
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    if !should_restart {
        return Ok(Json(serde_json::json!({"ok": true, "restarted": false})));
    }
    let pty = svc.pty.clone();
    let screen_state = svc.screen_state.clone();
    let config = state.config.clone();
    let app = svc.app_handle.clone();
    std::thread::spawn(move || {
        crate::service::general_agent::restart_general_agent(
            &svc,
            &mux,
            &pty,
            &screen_state,
            &config,
            &app,
        );
    });
    Ok(Json(serde_json::json!({"ok": true, "restarted": true})))
}

#[derive(serde::Deserialize)]
pub struct CompleteFirstRunBody {
    pub provider_id: String,
}

/// Web-mirror twin of `complete_general_agent_first_run`.
pub async fn complete_general_agent_first_run(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<CompleteFirstRunBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let app = state.svc.app_handle.clone();
    crate::service::general_agent::complete_first_run_core(
        &state.svc,
        &state.config,
        &app,
        &body.provider_id,
    )
    .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// Web-mirror twin of `install_provider_in_pane`.
pub async fn install_provider_in_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(provider_id): Path<String>,
) -> Result<Json<crate::commands::InstallPaneResult>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    crate::commands::install_provider_in_pane_core(&state.svc, provider_id)
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_body_minimal() {
        let body: SpawnAgentBody = serde_json::from_str(r#"{"agent":"claude"}"#).unwrap();
        assert_eq!(body.agent.as_deref(), Some("claude"));
    }

    #[test]
    fn spawn_body_empty_object() {
        let body: SpawnAgentBody = serde_json::from_str("{}").unwrap();
        assert_eq!(body.agent, None);
    }

    #[test]
    fn spawn_role_validation_treats_empty_strings_as_absent() {
        let role_probe = validate_spawn_role(Some("  ".into()), Some("".into())).unwrap();
        assert!(role_probe.role.is_none());
        assert!(role_probe.role_profile.is_none());
    }

    #[test]
    fn spawn_role_validation_rejects_unknown_role_with_body() {
        let (status, body) = validate_spawn_role(Some("Lead".into()), None).unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("Invalid role: Lead"));
        assert!(!body.is_empty());
    }

    // A plain-text (StatusCode, String) body used to render as bare "HTTP 500"
    // once relayed; this proves the structured shape now survives that hop.
    #[test]
    fn spawn_rejection_serializes_a_structured_error_not_bare_text() {
        let rejection = SpawnRejection::from("checkout guard rejected: dirty worktree".to_string());
        assert_eq!(rejection.status, StatusCode::INTERNAL_SERVER_ERROR);
        let error = rejection.body.get("error").expect("error object present");
        assert_eq!(
            error.get("code").and_then(|v| v.as_str()),
            Some("spawn_failed")
        );
        assert_eq!(
            error.get("message").and_then(|v| v.as_str()),
            Some("checkout guard rejected: dirty worktree")
        );
        assert!(
            error
                .get("retry_hint")
                .and_then(|v| v.as_str())
                .is_some_and(|hint| !hint.is_empty()),
            "a caller must get a next-step, not just a message"
        );
        assert_eq!(
            rejection.body.get("ok").and_then(|v| v.as_bool()),
            Some(false)
        );
    }

    struct ListingShell;

    impl crate::chatbackend::ChatBackendProvider for ListingShell {
        fn id(&self) -> &str {
            "local-shell"
        }

        fn open_session(
            &self,
            _: serde_json::Value,
        ) -> Result<String, crate::chatbackend::ChatBackendError> {
            Ok("session".into())
        }

        fn send_message(
            &self,
            _: &str,
            _: &str,
        ) -> Result<(), crate::chatbackend::ChatBackendError> {
            Ok(())
        }

        fn poll(
            &self,
            _: &str,
            _: Option<&str>,
        ) -> Result<crate::agent::chat_types::StructuredChat, crate::chatbackend::ChatBackendError>
        {
            Ok(crate::agent::chat_types::StructuredChat::default())
        }

        fn close_session(&self, _: &str) -> Result<(), crate::chatbackend::ChatBackendError> {
            Ok(())
        }

        fn list_models(
            &self,
        ) -> Result<Vec<crate::agent::provider::ModelInfo>, crate::chatbackend::ChatBackendError>
        {
            Ok(vec![crate::agent::provider::ModelInfo {
                id: "qwen".into(),
                display_name: "Qwen".into(),
                description: None,
                available: None,
            }])
        }

        fn display_name(&self) -> &str {
            "Local Shell"
        }

        fn spawnable(&self) -> bool {
            true
        }
    }

    /// A registered chatBackend that is NOT user-spawnable (bare
    /// `chatBackend: true`) — e.g. the NPC backend. Must be hidden from the
    /// new-session picker.
    struct BackendOnlyShell;

    impl crate::chatbackend::ChatBackendProvider for BackendOnlyShell {
        fn id(&self) -> &str {
            "npc"
        }

        fn open_session(
            &self,
            _: serde_json::Value,
        ) -> Result<String, crate::chatbackend::ChatBackendError> {
            Ok("session".into())
        }

        fn send_message(
            &self,
            _: &str,
            _: &str,
        ) -> Result<(), crate::chatbackend::ChatBackendError> {
            Ok(())
        }

        fn poll(
            &self,
            _: &str,
            _: Option<&str>,
        ) -> Result<crate::agent::chat_types::StructuredChat, crate::chatbackend::ChatBackendError>
        {
            Ok(crate::agent::chat_types::StructuredChat::default())
        }

        fn close_session(&self, _: &str) -> Result<(), crate::chatbackend::ChatBackendError> {
            Ok(())
        }

        fn list_models(
            &self,
        ) -> Result<Vec<crate::agent::provider::ModelInfo>, crate::chatbackend::ChatBackendError>
        {
            Ok(Vec::new())
        }
        // spawnable() defaults to false.
    }

    fn facade_with(
        shells: &[(
            &str,
            std::sync::Arc<dyn crate::chatbackend::ChatBackendProvider>,
        )],
    ) -> crate::provider::ProviderRegistry {
        let agents = std::sync::Arc::new(crate::agent::registry::AgentRegistry::new());
        let backends = std::sync::Arc::new(crate::chatbackend::ChatBackendRegistry::new());
        for (id, shell) in shells {
            backends.register(*id, shell.clone());
        }
        crate::provider::ProviderRegistry::new(agents, backends)
    }

    #[test]
    fn provider_listing_includes_spawnable_chatbackend_tagged_transport() {
        let registry = facade_with(&[("local-shell", std::sync::Arc::new(ListingShell))]);

        let listing = super::list_agent_provider_values(&registry);

        let shell = listing
            .iter()
            .find(|p| p.get("id").and_then(|v| v.as_str()) == Some("local-shell"))
            .expect("registered spawnable chatBackend shell is listed");
        // `kind` is gone — transport is the discriminant now.
        assert!(
            shell.get("kind").is_none(),
            "no `kind` key in serialized output"
        );
        assert_eq!(shell["transport"], "chat_backend");
        assert_eq!(shell["display_name"], "Local Shell");
        // list_models() getter, so this stays empty by design.
        assert_eq!(shell["available_models"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn provider_listing_hides_non_spawnable_chatbackends() {
        // in the new-session picker. Only spawnable shells are listed.
        let registry = facade_with(&[
            ("local-shell", std::sync::Arc::new(ListingShell)),
            ("npc", std::sync::Arc::new(BackendOnlyShell)),
        ]);

        let listing = super::list_agent_provider_values(&registry);

        assert!(
            listing
                .iter()
                .any(|p| p.get("id").and_then(|v| v.as_str()) == Some("local-shell")),
            "spawnable shell is listed"
        );
        assert!(
            !listing
                .iter()
                .any(|p| p.get("id").and_then(|v| v.as_str()) == Some("npc")),
            "non-spawnable backend must NOT be listed"
        );
    }
}
