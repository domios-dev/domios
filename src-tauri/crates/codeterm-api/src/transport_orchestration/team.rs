use super::{authority, AppState, RouteError};
use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use serde::de::DeserializeOwned;
use serde_json::Value;

fn attributed_body<T: DeserializeOwned>(
    state: &AppState,
    mut headers: HeaderMap,
    mut body: Value,
) -> Result<(HeaderMap, T), RouteError> {
    let claim: authority::CallerClaim = serde_json::from_value(body.clone())
        .map_err(|e| (axum::http::StatusCode::BAD_REQUEST, e.to_string()))?;
    let caller = authority::validate_claim(state, &headers, &claim)?;
    if let Some(caller) = caller {
        headers.insert(
            "X-CodeTerm-Tab-Id",
            caller.parse().map_err(|_| {
                (
                    axum::http::StatusCode::BAD_REQUEST,
                    "Invalid caller header".into(),
                )
            })?,
        );
    }
    if let Some(object) = body.as_object_mut() {
        object.remove("caller_tab_id");
        object.remove("session_binding_nonce");
    }
    let body = serde_json::from_value(body)
        .map_err(|e| (axum::http::StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok((headers, body))
}

pub(super) async fn rename(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<codeterm_types::team::Team>, RouteError> {
    let (headers, body) = attributed_body(&state, headers, body)?;
    super::super::team_routes::rename_team(State(state), headers, Path(id), Json(body)).await
}

pub(super) async fn close(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<codeterm_service_core::service::TeamCloseResult>, RouteError> {
    let (headers, body) = attributed_body(&state, headers, body)?;
    super::super::team_routes::close_team(
        State(state),
        headers,
        Path(id),
        codeterm_service_core::api_support::optional_json::OptionalJson(Some(body)),
    )
    .await
}

pub(super) async fn member(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<codeterm_service_core::service::AddTeamMemberResult>, RouteError> {
    let (headers, body) = attributed_body(&state, headers, body)?;
    super::super::team_routes::add_team_member(State(state), headers, Path(id), Json(body)).await
}

pub(super) async fn pair(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, crate::agent_routes::SpawnRejection> {
    let (headers, body) = attributed_body(&state, headers, body).map_err(|(status, message)| {
        crate::agent_routes::SpawnRejection::new(status, "bad_request", message)
    })?;
    super::super::agent_routes::create_orchestration_pair(State(state), headers, Json(body)).await
}

pub(super) async fn make_orchestrator(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, RouteError> {
    let (headers, _body): (_, Value) = attributed_body(&state, headers, body)?;
    super::super::agent_routes::make_orchestrator(State(state), headers, Path(id)).await
}

pub(super) async fn make_planner(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, RouteError> {
    let (headers, body) = attributed_body(&state, headers, body)?;
    super::super::agent_routes::make_planner(State(state), headers, Path(id), Json(body)).await
}
