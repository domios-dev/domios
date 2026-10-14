mod action;
mod cli;
mod diff_cache;
mod permissions;
mod snapshot;
mod view;

pub use action::run_action;
pub use action::PluginManagerAction;
pub use view::bubble_poll;
pub use view::glance_render;
pub use view::view_call;

pub async fn plugin_manager_snapshot_for_service(
    svc: crate::service::ServiceState,
) -> Result<serde_json::Value, String> {
    action::plugin_manager_snapshot_for_service(svc).await
}

pub fn invalidate_plugin_snapshot() {
    action::invalidate_plugin_snapshot();
}
