use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::api::routes::{check_auth, AppState};

pub async fn health(AxumState(state): AxumState<AppState>) -> Json<serde_json::Value> {
    match crate::dispatch::dispatch(&crate::dispatch::Command::Health, &state.svc) {
        Ok(r) => Json(r.into_json()),
        Err(_) => Json(serde_json::json!({"status": "error"})),
    }
}

pub async fn identify(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    Ok(Json(serde_json::json!({
        "name": "CodeTerm",
        "version": env!("CARGO_PKG_VERSION"),
        "platform": std::env::consts::OS,
        "features": ["multiplexer", "agent-detection", "notifications", "rest-api", "canvas"]
    })))
}
