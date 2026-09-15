use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::agent_routes::{verify_spawn_delivery, SpawnRejection};
use crate::api::routes::{check_auth, AppState};
use crate::service::ai_helper_spawn::AiHelperRequest;

/// Provider, workspace and cwd are daemon-owned, so a helper names only its work.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AiHelperSpawnBody {
    title: String,
    task: String,
}

pub async fn spawn_ai_helper(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<AiHelperSpawnBody>,
) -> Result<Json<serde_json::Value>, SpawnRejection> {
    check_auth(&state, &headers).map_err(|(status, message)| {
        SpawnRejection::new(status, "unauthorized", message.to_string())
    })?;
    let request = AiHelperRequest::new(body.title, body.task)
        .map_err(|message| SpawnRejection::new(StatusCode::BAD_REQUEST, "bad_request", message))?;
    let launch = state
        .svc
        .spawn_ai_helper(request)
        .await
        .map_err(SpawnRejection::from)?;
    if let Some(ready_rx) = launch.ready {
        verify_spawn_delivery(&state, &launch.tab_id, ready_rx).await?;
    }
    Ok(Json(serde_json::json!({
        "id": launch.tab_id,
        "tab_id": launch.tab_id,
        "workspace_id": launch.workspace_id,
        "delivery_state": launch.delivery.as_str(),
    })))
}

#[cfg(test)]
mod tests;
