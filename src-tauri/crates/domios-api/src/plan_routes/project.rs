use super::{text_err, PlanRouteResult};
use crate::routes::{check_auth, AppState};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct SetProjectBody {
    project_path: String,
}

pub async fn set_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<SetProjectBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(status, message)| text_err(status, message))?;
    let svc = state.svc.clone();
    let update = tokio::task::spawn_blocking(move || svc.set_plan_project(&id, &body.project_path))
        .await
        .map_err(|error| text_err(StatusCode::INTERNAL_SERVER_ERROR, error))?
        .map_err(|error| text_err(StatusCode::UNPROCESSABLE_ENTITY, error))?;
    serde_json::to_value(update)
        .map(Json)
        .map_err(|error| text_err(StatusCode::INTERNAL_SERVER_ERROR, error))
}
