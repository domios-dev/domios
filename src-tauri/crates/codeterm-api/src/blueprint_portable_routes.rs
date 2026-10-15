use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::Value;

use super::{
    auth::{caller_access_level, caller_peer_id},
    routes::AppState,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportBody {
    workspace_id: String,
    team_id: Value,
    id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportBody {
    caller_tab_id: String,
    recipe: codeterm_blueprint_types::blueprint_recipe::Recipe,
    assignments: Vec<String>,
}

fn host_access(state: &AppState, headers: &HeaderMap) -> Result<(), (StatusCode, String)> {
    caller_access_level(state, headers)?;
    if caller_peer_id(state, headers).is_some() {
        return Err((
            StatusCode::FORBIDDEN,
            "peer-origin Blueprint sharing is unavailable".into(),
        ));
    }
    Ok(())
}

pub async fn export(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ExportBody>,
) -> Result<Json<codeterm_blueprint_types::blueprint_recipe::Recipe>, (StatusCode, String)> {
    host_access(&state, &headers)?;
    let team = codeterm_service_core::service::sentinel::graph::explicit_team(&body.team_id)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    tokio::task::spawn_blocking(move || {
        state
            .svc
            .blueprint_export_host(&body.workspace_id, team.as_deref(), &body.id)
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map(Json)
    .map_err(|error| (StatusCode::BAD_REQUEST, error))
}

pub async fn import(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ImportBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    host_access(&state, &headers)?;
    tokio::task::spawn_blocking(move || {
        state
            .svc
            .blueprint_import_host(&body.caller_tab_id, body.recipe, &body.assignments)
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map(Json)
    .map_err(|error| (StatusCode::BAD_REQUEST, error))
}
