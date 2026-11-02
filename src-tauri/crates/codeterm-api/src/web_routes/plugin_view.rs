use crate::routes::AppState;
use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    Json,
};
use codeterm_commands::commands;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ViewCallReq {
    pub plugin_id: String,
    pub view_frame: String,
    pub method: String,
    #[serde(default)]
    pub args: Option<serde_json::Value>,
}

/// Web-mirror twin of the `plugin_view_call` Tauri command (the iframe bridge).
pub async fn plugin_view_call(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Json(req): Json<ViewCallReq>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    commands::plugins::view_call(
        &state.svc,
        &req.plugin_id,
        &req.method,
        req.args.unwrap_or(serde_json::Value::Null),
        &req.view_frame,
    )
    .await
    .map(Json)
    .map_err(|error| {
        let status = match error.as_str() {
            "plugin_view_frame_invalid" | "plugin_view_scope_invalid" => StatusCode::FORBIDDEN,
            "plugin_view_request_invalid" => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, error)
    })
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RemoteTaskReq {
    tab_id: String,
    plugin_id: String,
    session_id: String,
    method: String,
    #[serde(default)]
    args: Option<serde_json::Value>,
}

pub async fn remote_task_call(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Json(req): Json<RemoteTaskReq>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    commands::plugins::plugin_remote_task_call(
        &state.svc,
        &req.tab_id,
        &req.plugin_id,
        &req.session_id,
        &req.method,
        req.args.unwrap_or(serde_json::Value::Null),
    )
    .await
    .map(Json)
    .map_err(|error| (StatusCode::FORBIDDEN, error))
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RecordingReq {
    plugin_id: String,
    args: serde_json::Value,
}

pub async fn recording_suggestion(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Json(req): Json<RecordingReq>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    commands::plugins::plugin_recording_suggestion(&state.svc, &req.plugin_id, req.args)
        .await
        .map(Json)
        .map_err(|error| (StatusCode::FORBIDDEN, error))
}

#[cfg(test)]
mod tests;
