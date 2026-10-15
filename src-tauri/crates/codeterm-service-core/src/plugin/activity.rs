pub use codeterm_plugin::activity::*;

pub fn emit_plugin_activity(svc: &crate::service::ServiceContext, activity: &PluginActivity) {
    svc.ui.emit_native(
        "plugin-activity",
        serde_json::to_value(activity).unwrap_or_default(),
    );
    svc.events
        .publish(codeterm_events::SystemEvent::PluginActivity(
            activity.clone(),
        ));
}
