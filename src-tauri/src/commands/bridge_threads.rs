//! Tab-menu entrance to per-tab Telegram threads. The user acts on the tab's
//! own menu, so these calls carry owner authority into the same service op.

use tauri::State;

use crate::service::ServiceState;

#[tauri::command]
pub async fn bridge_thread_state(
    svc: State<'_, ServiceState>,
    tab_id: String,
) -> Result<serde_json::Value, String> {
    let svc = svc.inner().clone();
    let state = svc.bridge_thread_menu_state(&tab_id).await;
    serde_json::to_value(state).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn bridge_thread_open(
    svc: State<'_, ServiceState>,
    tab_id: String,
) -> Result<serde_json::Value, String> {
    let svc = svc.inner().clone();
    let opened = svc
        .open_bridge_thread(None, &tab_id, None)
        .await
        .map_err(|error| error.to_string())?;
    serde_json::to_value(opened).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn bridge_thread_close(
    svc: State<'_, ServiceState>,
    tab_id: String,
) -> Result<serde_json::Value, String> {
    let svc = svc.inner().clone();
    let outcomes = svc.close_bridge_thread(&tab_id).await;
    Ok(serde_json::json!({ "outcomes": outcomes }))
}
