use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::routes::{check_auth, AppState};

#[derive(Deserialize)]
pub(crate) struct SaveClipboardImageBody {
    bytes: Vec<u8>,
    ext: String,
}

pub async fn save_clipboard_image(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<SaveClipboardImageBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let path = codeterm_service_core::commands::save_clipboard_image_core(body.bytes, body.ext)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({ "path": path })))
}

#[derive(Deserialize)]
pub(crate) struct SaveClipboardFileBody {
    bytes: Vec<u8>,
    filename: String,
}

pub async fn save_clipboard_file(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<SaveClipboardFileBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let path = codeterm_service_core::commands::save_clipboard_file_core(body.bytes, body.filename)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({ "path": path })))
}
