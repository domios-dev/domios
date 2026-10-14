pub use codeterm_plugin::activity::*;

pub fn emit_plugin_activity(svc: &crate::service::ServiceContext, activity: &PluginActivity) {
    use tauri::Emitter;
    let _ = svc.app_handle.emit("plugin-activity", activity);
    svc.events
        .publish(codeterm_events::SystemEvent::PluginActivity(
            activity.clone(),
        ));
}
