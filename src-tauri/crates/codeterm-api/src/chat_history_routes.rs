use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::routes::{check_auth, AppState};

#[derive(Deserialize)]
pub struct ChatHistoryBody {
    source: codeterm_service_core::agent::chat_types::ChatHistorySource,
    session_id: String,
    before_offset: u64,
}

/// One older page behind a history gap row; the session must belong to the tab.
pub async fn chat_raw_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tab_id): Path<String>,
    Json(body): Json<ChatHistoryBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let svc = state.svc.clone();
    let messages = tokio::task::spawn_blocking(move || {
        codeterm_service_core::service::tab_chat::history_gap::page(
            &svc,
            &tab_id,
            body.source,
            &body.session_id,
            body.before_offset,
        )
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| (StatusCode::CONFLICT, e))?;
    Ok(Json(serde_json::json!({ "messages": messages })))
}
