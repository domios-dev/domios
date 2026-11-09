//! A client records its own selection; other clients never hear of it.

use super::routes::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::post,
    Json, Router,
};

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/workspaces/{id}/select", post(select_workspace))
        .route(
            "/api/workspaces/{id}/tabs/{tab_id}/select",
            post(select_workspace_tab),
        )
        .route("/api/tabs/{id}/select", post(select_tab))
}

type Selected = Result<Json<serde_json::Value>, (StatusCode, String)>;

async fn record(state: AppState, workspace: Option<String>, tab: Option<String>) -> Selected {
    tokio::task::spawn_blocking(move || {
        state
            .svc
            .record_client_selection(workspace.as_deref(), tab.as_deref())
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| (StatusCode::NOT_FOUND, error))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn select_workspace(State(state): State<AppState>, Path(id): Path<String>) -> Selected {
    record(state, Some(id), None).await
}

async fn select_workspace_tab(
    State(state): State<AppState>,
    Path((id, tab_id)): Path<(String, String)>,
) -> Selected {
    record(state, Some(id), Some(tab_id)).await
}

async fn select_tab(State(state): State<AppState>, Path(id): Path<String>) -> Selected {
    record(state, None, Some(id)).await
}

#[cfg(test)]
mod tests;
