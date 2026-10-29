mod action;
mod cli;
mod diff_cache;
mod host_action;
mod permissions;
mod snapshot;
mod view;

pub use action::run_action;
pub use action::PluginManagerAction;
pub use host_action::{view_host_action, view_host_download, HostActionRequest};
pub use view::bubble_poll;
pub use view::glance_render;
pub use view::view_call;

pub async fn plugin_manager_snapshot_for_service(
    svc: codeterm_service_core::service::ServiceState,
    lifetime: impl Send + Sync + 'static,
) -> Result<serde_json::Value, String> {
    action::plugin_manager_snapshot_for_service(svc, lifetime).await
}

pub fn invalidate_plugin_snapshot() {
    action::invalidate_plugin_snapshot();
}

pub use view::host::{
    recording_suggestion as plugin_recording_suggestion,
    remote_task_call as plugin_remote_task_call,
};
