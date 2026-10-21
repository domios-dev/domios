pub mod discovery;
pub mod shutdown;

pub fn bind_status_emitter(ui: std::sync::Arc<dyn codeterm_service_core::ports::host::UiEvents>) {
    let _ = codeterm_update::update_state::STATUS_EMITTER.set(Box::new(move |status| {
        ui.emit(
            "update-status-changed",
            serde_json::to_value(status).unwrap_or(serde_json::Value::Null),
        );
    }));
}
