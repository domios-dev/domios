use super::{
    auth::{caller_access_level, caller_peer_id},
    routes::AppState,
};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use codeterm_sentinel_host::BlueprintMemoryExt;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SolutionBody {
    workspace_id: String,
    team_id: Value,
    id: String,
    when_to_use: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListBody {
    workspace_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShowBody {
    workspace_id: String,
    id: String,
}

fn access(state: &AppState, headers: &HeaderMap) -> Result<(), (StatusCode, String)> {
    caller_access_level(state, headers)?;
    if caller_peer_id(state, headers).is_some() {
        return Err((
            StatusCode::FORBIDDEN,
            "peer-origin saved solutions are unavailable".into(),
        ));
    }
    Ok(())
}
fn confirmed(headers: &HeaderMap, id: &str) -> Result<(), (StatusCode, String)> {
    if headers.contains_key("x-codeterm-tab-id")
        || headers
            .get("x-codeterm-blueprint-confirmation")
            .and_then(|value| value.to_str().ok())
            != Some(id)
    {
        return Err((
            StatusCode::FORBIDDEN,
            "Solution mutation requires explicit human confirmation without a tab identity".into(),
        ));
    }
    Ok(())
}
pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ListBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    access(&state, &headers)?;
    tokio::task::spawn_blocking(move || state.svc.blueprint_solutions_host(&body.workspace_id))
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map(Json)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))
}
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ShowBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    access(&state, &headers)?;
    tokio::task::spawn_blocking(move || {
        state
            .svc
            .blueprint_solution_show_host(&body.workspace_id, &body.id)
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map(Json)
    .map_err(|error| (StatusCode::BAD_REQUEST, error))
}
pub async fn save(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SolutionBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    access(&state, &headers)?;
    confirmed(&headers, &body.id)?;
    let team = codeterm_sentinel_host::service_adapter::graph::explicit_team(&body.team_id)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    tokio::task::spawn_blocking(move || {
        state.svc.blueprint_save_solution_host(
            &body.workspace_id,
            team.as_deref(),
            &body.id,
            body.when_to_use.as_deref().unwrap_or_default(),
            &body.tags,
        )
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map(Json)
    .map_err(|error| (StatusCode::BAD_REQUEST, error))
}
pub async fn archive(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SolutionBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    access(&state, &headers)?;
    confirmed(&headers, &body.id)?;
    tokio::task::spawn_blocking(move || {
        state
            .svc
            .blueprint_archive_solution_host(&body.workspace_id, &body.id)
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map(Json)
    .map_err(|error| (StatusCode::BAD_REQUEST, error))
}

#[cfg(test)]
mod tests;
