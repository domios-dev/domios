use axum::{http::StatusCode, Json};

use crate::routes::AppState;

pub(crate) async fn close_remote_tab(
    state: &AppState,
    peer_id: &str,
    controller_tab_id: Option<&str>,
    tab_id: &str,
    force: bool,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let service = state.svc.clone();
    let peer_id = peer_id.to_owned();
    let controller_tab_id = controller_tab_id.map(str::to_owned);
    let tab_id = tab_id.to_owned();
    let warnings = tokio::task::spawn_blocking(move || {
        service.close_tab_from_authenticated_mesh_caller(
            &peer_id,
            controller_tab_id.as_deref(),
            &tab_id,
            force,
        )
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(super::map_close_error)?;
    let mut result = serde_json::json!({"ok": true});
    if !warnings.is_empty() {
        result["warnings"] = serde_json::json!(warnings);
    }
    Ok(Json(result))
}
