use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::routes::AppState;

#[derive(Deserialize)]
pub struct SaveClipboardImageBody {
    bytes: Vec<u8>,
    ext: String,
}

pub async fn save_clipboard_image(
    AxumState(_state): AxumState<AppState>,
    _headers: HeaderMap,
    Json(body): Json<SaveClipboardImageBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || {
        let path = codeterm_commands::commands::save_clipboard_image_core(body.bytes, body.ext)
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
        Ok(Json(serde_json::json!({ "path": path })))
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

#[derive(Deserialize)]
pub struct SaveClipboardFileBody {
    bytes: Vec<u8>,
    filename: String,
}

pub async fn save_clipboard_file(
    AxumState(_state): AxumState<AppState>,
    _headers: HeaderMap,
    Json(body): Json<SaveClipboardFileBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || {
        let path = codeterm_commands::commands::save_clipboard_file_core(body.bytes, body.filename)
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
        Ok(Json(serde_json::json!({ "path": path })))
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}
