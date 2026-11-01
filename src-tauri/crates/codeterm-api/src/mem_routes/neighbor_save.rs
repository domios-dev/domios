use crate::routes::AppState;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use codeterm_service_core::mem_ops::neighbor_save::NeighborSaveBody;

pub async fn save(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<NeighborSaveBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let failure = |message: String| {
        let code = message
            .split(':')
            .next()
            .unwrap_or("neighbor_memory_write_not_allowed");
        (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"code":code,"reason":code,"message":message})),
        )
    };
    let caller =
        crate::auth::tab_caller(&state, &headers).map_err(|(_, message)| failure(message))?;
    let author = super::memory_author(&state, &headers)
        .await
        .map_err(|(_, message)| failure(message))?;
    state
        .svc
        .save_neighbor_memory(caller, author, body)
        .await
        .map(Json)
        .map_err(failure)
}
