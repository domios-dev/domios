use super::{
    auth::{caller_access_level, caller_peer_id},
    routes::AppState,
};
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use codeterm_sentinel_host::blueprint_viewer::BlueprintViewerExt;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryQuery {
    workspace_id: String,
    team_id: String,
    id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectQuery {
    workspace_id: String,
    path: Option<String>,
}
fn access(state: &AppState, headers: &HeaderMap) -> Result<(), (StatusCode, String)> {
    caller_access_level(state, headers)?;
    if caller_peer_id(state, headers).is_some() {
        return Err((
            StatusCode::FORBIDDEN,
            "Peer-origin Blueprint viewing is unavailable".into(),
        ));
    }
    Ok(())
}
pub async fn history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<Value>, (StatusCode, String)> {
    access(&state, &headers)?;
    tokio::task::spawn_blocking(move || {
        state.svc.blueprint_viewer_history(
            &query.workspace_id,
            (!query.team_id.is_empty()).then_some(query.team_id.as_str()),
            &query.id,
        )
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map(Json)
    .map_err(|e| (StatusCode::BAD_REQUEST, e))
}
pub async fn project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ProjectQuery>,
) -> Result<Json<Value>, (StatusCode, String)> {
    access(&state, &headers)?;
    tokio::task::spawn_blocking(move || {
        state
            .svc
            .blueprint_project_host(&query.workspace_id, query.path.as_deref())
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map(Json)
    .map_err(|e| (StatusCode::BAD_REQUEST, e))
}
