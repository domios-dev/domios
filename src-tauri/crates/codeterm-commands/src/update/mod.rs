pub mod apply;
pub mod channel;
pub mod checker;
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
