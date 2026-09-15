use serde_json::Value;

use crate::service::ServiceState;

pub(super) async fn plugin_tab_menu_impl(
    svc: tauri::State<'_, ServiceState>,
) -> Result<Value, String> {
    serde_json::to_value(svc.tab_menu_state()).map_err(|error| error.to_string())
}

pub(super) async fn plugin_tab_menu_action_impl(
    svc: tauri::State<'_, ServiceState>,
    plugin_id: String,
    item_id: String,
    tab_id: String,
) -> Result<Value, String> {
    let svc = svc.inner().clone();
    let state =
        tokio::task::spawn_blocking(move || svc.run_tab_menu_item(&plugin_id, &item_id, &tab_id))
            .await
            .map_err(|error| format!("tab menu action did not complete: {error}"))?
            .map_err(|error| error.message().to_string())?;
    serde_json::to_value(state).map_err(|error| error.to_string())
}
