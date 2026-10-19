pub mod apply;
pub mod channel;
pub mod checker;
#[cfg(any(test, feature = "desktop"))]
mod debian_verify;
pub mod discovery;
pub mod failure_report;
mod install_holders;
pub mod manifest;
pub mod orchestrate;
pub mod shutdown;
pub mod single_flight;
pub mod viability;

pub use channel::Channel;
pub use codeterm_service_core::update_state::{InstallMethod, UpdatePhase, UpdateState};
pub use manifest::Manifest;

pub fn bind_status_emitter(ui: std::sync::Arc<dyn codeterm_service_core::ports::host::UiEvents>) {
    let _ = codeterm_service_core::update_state::STATUS_EMITTER.set(Box::new(move |status| {
        ui.emit(
            "update-status-changed",
            serde_json::to_value(status).unwrap_or(serde_json::Value::Null),
        );
    }));
}

/// The split native client has no service UI bus; its frontend reads updater state from command replies.
#[cfg(feature = "desktop")]
pub fn emit_update_event(
    app: &codeterm_host_runtime::app_runtime::AppHandle,
    event: &str,
    payload: serde_json::Value,
) {
    use tauri::Manager;
    if let Some(service) = app.try_state::<codeterm_service_core::service::ServiceState>() {
        service.ui.emit(event, payload);
    }
}
