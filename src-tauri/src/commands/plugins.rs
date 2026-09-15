mod action;
mod cli;
mod diff_cache;
mod snapshot;
mod tab_menu;
mod view;

pub(crate) use action::{run_action, PluginManagerAction};
pub(crate) use view::{bubble_poll, glance_render, view_call};

pub(crate) async fn plugin_manager_snapshot_for_service(
    svc: crate::service::ServiceState,
) -> Result<serde_json::Value, String> {
    action::plugin_manager_snapshot_for_service(svc).await
}

pub(crate) fn invalidate_plugin_snapshot() {
    action::invalidate_plugin_snapshot();
}

#[tauri::command]
pub async fn plugin_manager_snapshot(
    svc: tauri::State<'_, crate::service::ServiceState>,
) -> Result<serde_json::Value, String> {
    action::plugin_manager_snapshot_impl(svc).await
}

#[tauri::command]
pub async fn plugin_manager_action(
    svc: tauri::State<'_, crate::service::ServiceState>,
    action: PluginManagerAction,
) -> Result<serde_json::Value, String> {
    action::plugin_manager_action_impl(svc, action).await
}

#[tauri::command]
pub async fn plugin_glance(
    svc: tauri::State<'_, crate::service::ServiceState>,
    plugin_id: String,
    action: Option<String>,
    ctx: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    view::plugin_glance_impl(svc, plugin_id, action, ctx).await
}

#[tauri::command]
pub async fn plugin_bubble(
    svc: tauri::State<'_, crate::service::ServiceState>,
    plugin_id: String,
    ctx: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    view::plugin_bubble_impl(svc, plugin_id, ctx).await
}

#[tauri::command]
pub async fn plugin_view_call(
    svc: tauri::State<'_, crate::service::ServiceState>,
    plugin_id: String,
    method: String,
    args: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    view::plugin_view_call_impl(svc, plugin_id, method, args).await
}

/// Manifest tab menu items plus the receiver bindings they toggle.
#[tauri::command]
pub async fn plugin_tab_menu(
    svc: tauri::State<'_, crate::service::ServiceState>,
) -> Result<serde_json::Value, String> {
    tab_menu::plugin_tab_menu_impl(svc).await
}

/// Runs the host-owned action of one declared item against one tab.
#[tauri::command]
pub async fn plugin_tab_menu_action(
    svc: tauri::State<'_, crate::service::ServiceState>,
    plugin_id: String,
    item_id: String,
    tab_id: String,
) -> Result<serde_json::Value, String> {
    tab_menu::plugin_tab_menu_action_impl(svc, plugin_id, item_id, tab_id).await
}
