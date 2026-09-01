use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

use crate::api::{
    optional_json::OptionalJson,
    routes::{check_auth, resolve_tab, AppState},
};
use crate::multiplexer::types::AgentRole;
use codeterm_types::agent::role_kind::builtins::PLANNER;

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
    parent_tab_id: Option<String>,
    #[serde(alias = "plan")]
    plan_id: Option<String>,
    can_spawn: Option<bool>,
    task: Option<String>,
    task_id: Option<String>,
    #[serde(alias = "plan-task")]
    plan_task: Option<String>,
    execution_intent: Option<crate::plan::ExecutionIntent>,
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
    #[serde(alias = "system")]
    system_prompt: Option<String>,
    preset: Option<String>,
    charter: Option<String>,
    tick_interval_secs: Option<u64>,
    nudge_cooldown_secs: Option<u64>,
    max_actions_per_tick: Option<u32>,
    activate: Option<bool>,
    /// Audience for input-request notifications. `automation` declares a tab
    /// (bench/probe) whose prompts have no human owner and must never bell.
    #[serde(default)]
    audience: crate::multiplexer::types::TabAudience,
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
pub(crate) struct RecordRemoteWorkerBody {
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
    let orchestrator =
        resolve_tab(&state, &body.orchestrator_tab).map_err(|(s, m)| (s, m.to_string()))?;
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
pub(crate) struct ForgetRemoteWorkerBody {
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
    let orchestrator = body
        .orchestrator_tab
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|p| resolve_tab(&state, p).map_err(|(s, m)| (s, m.to_string())))
        .transpose()?;
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
    let starter_prompt = crate::service::starter_prompt_for_role(role_probe.role.as_deref());
    let params = crate::service::SpawnAgentParams {
        agent: body.agent,
        agent_name: body.agent_name,
        args: body.args,
        cwd: body.cwd,
        provisioned_cwd: None,
        lane_setup_command: None,
        title: body.title,
        title_provenance: Some(crate::service::SpawnIdentityProvenance::UserSupplied),
        description: body.description,
        role: role_probe.role,
        role_profile: role_probe.role_profile,
        parent_tab_id: body.parent_tab_id,
        plan_id: body.plan_id,
        can_spawn: body.can_spawn,
        task: body.task,
        starter_prompt,
        task_id: body.task_id,
        task_launch_authority: None,
        plan_skill_manifest: None,
        plan_task: body.plan_task,
        execution_intent: body.execution_intent,
        provider_reserve_pct: body.provider_reserve_pct,
        workspace_id: body.workspace_id,
        force_tab: body.force_tab,
        existing_tab_id: None,
        skip_orchestrator_onboarding: None,
        report_target_tab_id: body.report_to.or(body.report_target_tab_id),
        report_target_mesh_peer: body.origin_peer,
        skip_permissions: None,
        permission_bypass_authorization: None,
        spawned_by_tab_id: body.spawned_by_tab_id,
        team_id: body.team_id,
        creator_tab_id: body.creator_tab_id,
        ownership: body.ownership,
        model: body.model,
        planned_model: None,
        planned_reasoning_effort: None,
        reasoning_effort: body.reasoning_effort,
        system_prompt: body.system_prompt,
        preset: body.preset,
        resume_session_id: None,
        charter: body.charter,
        tick_interval_secs: body.tick_interval_secs,
        nudge_cooldown_secs: body.nudge_cooldown_secs,
        max_actions_per_tick: body.max_actions_per_tick,
        activate: body.activate,
        audience: body.audience,
        manager_bootstrap: None,
        structured_prompt_server: None,
    };
    let plan_task = params.plan_task.clone();
    let result = state
        .svc
        .spawn_agent(params, None)
        .await
        .map_err(SpawnRejection::from)?;
    let ack_warning = if let Some(task_ref) = plan_task.as_deref() {
        match state.svc.arm_bound_worker_ack_deadline_for_tab(
            &result.tab_id,
            format!("codeterm agent spawn --plan-task {task_ref}"),
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
    if let Some(ref warning) = result.warning {
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
/// action. The web-mirror twin of `codeterm team pair` — both call
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
    parent_tab_id: Option<String>,
    can_spawn: Option<bool>,
}

pub async fn set_tab_role(
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
    let (parsed_role, parsed_profile) =
        crate::service::ServiceContext::parse_role_kind(&role, role_probe.role_profile.as_deref())
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    // Same contract as `agent spawn --role Planner` — never an anchor, so
    // promotion onboarding never applies here.
    if parsed_role == AgentRole::Manager && parsed_profile.as_deref() == Some(PLANNER) {
        let plan_id = body.plan_id.clone().ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "Planner requires a plan_id".to_string(),
            )
        })?;
        state
            .svc
            .place_planner_on_plan(&id, &plan_id, body.parent_tab_id.as_deref(), true)
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
        return Ok(Json(serde_json::json!({"ok": true})));
    }
    let becomes_anchor = codeterm_types::agent::role_kind::anchors_team(
        Some(parsed_role),
        parsed_profile.as_deref(),
    );
    let was_orchestrator = {
        let mux = state.svc.mux.read();
        mux.get_tab(&id).map(|p| p.anchors_team()).unwrap_or(false)
    };
    let plan_binding: Option<String> = None;
    let destination = match parsed_role {
        AgentRole::Manager if becomes_anchor => crate::service::AgentPlacement::Manager {
            role_profile: parsed_profile.clone().unwrap_or_default(),
            parent_tab_id: None,
            can_spawn: body.can_spawn,
        },
        AgentRole::Worker => crate::service::AgentPlacement::Worker {
            parent_tab_id: body.parent_tab_id.ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    "Worker requires a parent_tab_id".to_string(),
                )
            })?,
            role_profile: parsed_profile.clone(),
        },
        AgentRole::Manager => crate::service::AgentPlacement::Manager {
            parent_tab_id: Some(body.parent_tab_id.clone().ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    "Manager requires a parent_tab_id".to_string(),
                )
            })?),
            role_profile: parsed_profile.clone().unwrap_or_default(),
            can_spawn: body.can_spawn,
        },
    };
    state
        .svc
        .transition_agent(crate::service::AgentTransitionRequest {
            tab_id: id.clone(),
            destination,
            plan_binding,
            require_verified_session: true,
            planner_replacement: None,
            plan_rebind: None,
        })
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    if becomes_anchor {
        state
            .svc
            .maybe_deliver_orchestrator_promotion_onboarding(&id, was_orchestrator);
    }
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn clear_tab_role(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    state
        .svc
        .clear_tab_role(&id)
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
            Some(crate::plan::PlannerRef::Tab {
                tab_id: id,
                session_id: None,
                title: None,
            }),
        )
        .map_err(|error| (StatusCode::CONFLICT, error))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn get_tab_status(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let tab_id = resolve_tab(&state, &id).map_err(|(s, m)| (s, m.to_string()))?;
    let info = state
        .svc
        .get_tab_status(&tab_id)
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
        "target_tab_id": result.orchestrator_tab_id,
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

fn agent_restart_response(
    outcome: &crate::service::agent_restart::RestartOutcome,
) -> serde_json::Value {
    serde_json::json!({
        "restart_accepted": outcome.restart_accepted,
        "old_pty_torn_down": outcome.old_pty_torn_down,
        "agent_ready": outcome.agent_ready,
        "old_pty_id": outcome.old_pty_id,
        "new_pty_id": outcome.new_pty_id,
        "tab_id": outcome.tab_id,
        "provider": outcome.provider,
        "session_id": outcome.session_id,
        "session_was_stale": outcome.session_was_stale,
        "session_action": outcome.session_action(),
        "full_access": outcome.auto_approve,
        "force": outcome.force,
    })
}

fn agent_restart_failure_response(
    outcome: &crate::service::agent_restart::RestartOutcome,
    reason: &str,
) -> serde_json::Value {
    let mut response = agent_restart_response(outcome);
    response["error"] = serde_json::Value::String(reason.to_string());
    response
}

pub async fn agent_restart_handler(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
    OptionalJson(body): OptionalJson<AgentRestartBody>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if let Err((status, msg)) = check_auth(&state, &headers) {
        return (status, msg.to_string()).into_response();
    }
    let tab_id = match resolve_tab(&state, &raw_tab_id) {
        Ok(p) => p,
        Err((status, msg)) => return (status, msg.to_string()).into_response(),
    };
    let body = body.unwrap_or(AgentRestartBody {
        force: false,
        grace_secs: default_grace_secs(),
    });

    let svc = state.svc.clone();
    let force = body.force;
    let grace_secs = body.grace_secs;
    let attempt =
        tokio::task::spawn_blocking(move || svc.agent_restart_observed(&tab_id, force, grace_secs))
            .await;
    let attempt = match attempt {
        Ok(attempt) => attempt,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("restart worker failed: {error}"),
                    "restart_accepted": false,
                    "old_pty_torn_down": false,
                    "agent_ready": false,
                })),
            )
                .into_response();
        }
    };

    match attempt {
        Ok(outcome) => {
            (StatusCode::ACCEPTED, Json(agent_restart_response(&outcome))).into_response()
        }
        Err(crate::service::agent_restart::RestartAttemptError::Failed {
            outcome,
            reason,
            timed_out,
        }) => (
            if timed_out {
                StatusCode::GATEWAY_TIMEOUT
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            },
            Json(agent_restart_failure_response(&outcome, &reason)),
        )
            .into_response(),
        Err(crate::service::agent_restart::RestartAttemptError::Request(
            crate::service::agent_restart::AgentRestartError::NotFound(m),
        )) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": m}))).into_response(),
        Err(crate::service::agent_restart::RestartAttemptError::Request(
            crate::service::agent_restart::AgentRestartError::BadRequest(m),
        )) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": m})),
        )
            .into_response(),
        Err(crate::service::agent_restart::RestartAttemptError::Request(
            crate::service::agent_restart::AgentRestartError::RateLimited { retry_after_secs },
        )) => {
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
        Err(crate::service::agent_restart::RestartAttemptError::Request(
            crate::service::agent_restart::AgentRestartError::Internal(m),
        )) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": m})),
        )
            .into_response(),
    }
}

pub async fn respond_prompt(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let tab_id = resolve_tab(&state, &raw_tab_id).map_err(|(s, m)| (s, m.to_string()))?;
    let value = body
        .get("value")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Missing 'value' field".to_string()))?
        .to_string();
    let svc = state.svc.clone();
    let pid = tab_id.clone();
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
    Path(raw_tab_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let tab_id = resolve_tab(&state, &raw_tab_id).map_err(|(s, m)| (s, m.to_string()))?;
    state
        .svc
        .clear_prompt(&tab_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// Set agent type on a tab (enables chat view for that agent's provider).
pub async fn set_agent_type(
    AxumState(state): AxumState<AppState>,
    Path(tab_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let agent_type = body
        .get("agent_type")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    state.svc.set_tab_agent_type(&tab_id, agent_type);
    state.svc.emit_state_changed();
    Ok(Json(serde_json::json!({"ok": true})))
}

/// List all registered agent providers as `ProviderDescriptor`s.
pub async fn list_agent_providers(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    list_agent_providers_blocking(state.svc.provider_registry.clone()).await
}

async fn list_agent_providers_blocking(
    registry: std::sync::Arc<crate::provider::ProviderRegistry>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || {
        Json(serde_json::Value::Array(list_agent_provider_values(
            &registry,
        )))
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

pub async fn restart_general_agent_tab(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    axum::extract::Path(tab_id): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let svc = state.svc.clone();
    let mux = svc.mux.clone();
    let should_restart =
        crate::service::general_agent::claim_general_agent_tab_retry(&tab_id, &mux)
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
    pub full_access: bool,
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
        body.full_access,
    )
    .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// Web-mirror twin of `install_provider_in_tab`.
pub async fn install_provider_in_tab(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(provider_id): Path<String>,
) -> Result<Json<crate::commands::InstallTabResult>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    crate::commands::install_provider_in_tab_core(&state.svc, provider_id)
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

#[cfg(test)]
mod restart_tests;

#[cfg(test)]
mod tests;
