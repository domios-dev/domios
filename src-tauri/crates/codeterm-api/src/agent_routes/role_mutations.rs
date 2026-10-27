use super::*;
use codeterm_service_core::multiplexer::types::AgentRole;
use codeterm_service_core::service::tab_mutation_authority::TabMutationScope;
use codeterm_types::agent::role_kind::builtins::PLANNER;

#[derive(Deserialize)]
pub struct SetRoleBody {
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
    tokio::task::spawn_blocking(move || {
        check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
        mutation_authority::authorize(&state, &headers, &id, Default::default())?;
        let mut role_probe = codeterm_service_core::service::SpawnAgentParams {
            role: Some(body.role),
            role_profile: body.role_profile.clone(),
            ..Default::default()
        };
        role_probe
            .normalize_role_profile()
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
        let role = role_probe.role.unwrap_or_else(|| "Worker".into());
        let (parsed_role, parsed_profile) =
            codeterm_service_core::service::ServiceContext::parse_role_kind(
                &role,
                role_probe.role_profile.as_deref(),
            )
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
            mutation_authority::authorize(
                &state,
                &headers,
                &id,
                TabMutationScope {
                    parent_tab_id: body.parent_tab_id.as_deref(),
                    plan_id: Some(&plan_id),
                    creates_team: false,
                },
            )?;
            state
                .svc
                .place_planner_on_plan(&id, &plan_id, body.parent_tab_id.as_deref(), true)
                .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
            return Ok(Json(serde_json::json!({"ok": true})));
        }
        if let Some(outcome) = worker_transfer::try_task_bound_worker_transfer(
            &state.svc,
            &headers,
            &id,
            parsed_role,
            body.role_profile.as_deref(),
            body.plan_id.as_deref(),
            body.parent_tab_id.as_deref(),
            body.can_spawn,
        )
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?
        {
            return Ok(Json(outcome));
        }
        let becomes_anchor = codeterm_types::agent::role_kind::anchors_team(
            Some(parsed_role),
            parsed_profile.as_deref(),
        );
        mutation_authority::authorize(
            &state,
            &headers,
            &id,
            TabMutationScope {
                parent_tab_id: body.parent_tab_id.as_deref(),
                plan_id: body.plan_id.as_deref(),
                creates_team: becomes_anchor,
            },
        )?;
        let was_orchestrator = {
            let mux = state.svc.mux.read();
            mux.get_tab(&id).map(|p| p.anchors_team()).unwrap_or(false)
        };
        if becomes_anchor {
            codeterm_orchestration::admit_anchor_promotion(&state.svc, &id)
                .map_err(|error| (StatusCode::FORBIDDEN, error))?;
        }
        let plan_binding: Option<String> = None;
        let destination = match parsed_role {
            AgentRole::Manager if becomes_anchor => {
                codeterm_service_core::service::AgentPlacement::Manager {
                    role_profile: parsed_profile.clone().unwrap_or_default(),
                    parent_tab_id: None,
                    can_spawn: body.can_spawn,
                }
            }
            AgentRole::Worker => codeterm_service_core::service::AgentPlacement::Worker {
                parent_tab_id: body.parent_tab_id.ok_or_else(|| {
                    (
                        StatusCode::BAD_REQUEST,
                        "Worker requires a parent_tab_id".to_string(),
                    )
                })?,
                role_profile: parsed_profile.clone(),
            },
            AgentRole::Manager => codeterm_service_core::service::AgentPlacement::Manager {
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
            .transition_agent(codeterm_service_core::service::AgentTransitionRequest {
                tab_id: id.clone(),
                destination,
                plan_binding,
                require_verified_session: true,
                planner_replacement: None,
                plan_rebind: None,
            })
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
        if becomes_anchor {
            codeterm_orchestration::maybe_deliver_orchestrator_promotion_onboarding(
                &state.svc,
                &id,
                was_orchestrator,
            );
        }
        Ok(Json(serde_json::json!({"ok": true})))
    })
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "blocking task failed".to_string(),
        )
    })?
}

pub async fn clear_tab_role(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || {
        check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
        mutation_authority::authorize(&state, &headers, &id, Default::default())?;
        state
            .svc
            .clear_tab_role(&id)
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
        Ok(Json(serde_json::json!({"ok": true})))
    })
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "blocking task failed".to_string(),
        )
    })?
}

pub async fn make_orchestrator(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || {
        check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
        mutation_authority::authorize(
            &state,
            &headers,
            &id,
            TabMutationScope {
                creates_team: true,
                ..Default::default()
            },
        )?;
        codeterm_orchestration::make_orchestrator(&state.svc, &id)
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
        Ok(Json(serde_json::json!({"ok": true})))
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
pub struct MakePlannerBody {
    plan_id: String,
}

pub async fn make_planner(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<MakePlannerBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || {
        check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
        mutation_authority::authorize(
            &state,
            &headers,
            &id,
            TabMutationScope {
                plan_id: Some(&body.plan_id),
                ..Default::default()
            },
        )?;
        state
            .svc
            .transition_plan_planner(
                &body.plan_id,
                Some(codeterm_service_core::plan::PlannerRef::Tab {
                    tab_id: id,
                    session_id: None,
                    title: None,
                }),
            )
            .map_err(|error| (StatusCode::CONFLICT, error))?;
        Ok(Json(serde_json::json!({"ok": true})))
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
pub struct CanSpawnBody {
    can_spawn: bool,
}

pub async fn set_can_spawn(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<CanSpawnBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || {
        check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
        mutation_authority::authorize(&state, &headers, &id, Default::default())?;
        state
            .svc
            .set_can_spawn(&id, body.can_spawn)
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
        Ok(Json(serde_json::json!({"ok": true})))
    })
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "blocking task failed".to_string(),
        )
    })?
}
