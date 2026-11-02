use crate::routes::{check_auth, AppState};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use codeterm_service_core::service::AuthenticatedPlanCaller;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
pub struct AdoptRemoteBody {
    tab_id: String,
    peer: String,
}

#[derive(Deserialize)]
pub struct ReparentBody {
    tab_id: String,
    to: String,
    peer: Option<String>,
}

fn local_caller(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<AuthenticatedPlanCaller, (StatusCode, String)> {
    check_auth(state, headers).map_err(|(status, message)| (status, message.into()))?;
    if !crate::auth::is_local_caller(state, headers) {
        return Err((
            StatusCode::FORBIDDEN,
            "remote Team adoption and reparenting must originate locally".into(),
        ));
    }
    Ok(crate::tab_routes::authenticated_plan_caller(headers))
}

pub async fn adopt_remote(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(team): Path<String>,
    Json(body): Json<AdoptRemoteBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let caller = local_caller(&state, &headers)?;
    state
        .svc
        .adopt_remote_team_member(caller, &team, &body.tab_id, &body.peer)
        .await
        .map(Json)
        .map_err(|error| (StatusCode::CONFLICT, error))
}

pub async fn reparent(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ReparentBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let caller = local_caller(&state, &headers)?;
    state
        .svc
        .reparent_worker(caller, body.tab_id, body.to, body.peer)
        .await
        .map(Json)
        .map_err(|error| (StatusCode::CONFLICT, error))
}

pub async fn inventory(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(team): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, message)| (status, message.into()))?;
    let service = state.svc.clone();
    tokio::task::spawn_blocking(move || {
        service
            .team_member_inventory(&team)
            .map(Json)
            .map_err(|error| (StatusCode::CONFLICT, error))
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}
