mod action;
mod cli;
mod diff_cache;
mod permissions;
mod snapshot;
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
