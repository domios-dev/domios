use crate::routes::AppState;

pub(super) fn record_hook_post(
    state: &AppState,
    tab_id: &str,
    event: &str,
    body: Option<&serde_json::Value>,
) {
    let enabled = state.config.lock().diagnostics.protocol_recording;
    let empty = serde_json::Value::Null;
    let _ = state
        .svc
        .ext::<std::sync::Arc<codeterm_service_core::service::ProtocolRecorder>>()
        .try_record_hook_post(enabled, tab_id, event, body.unwrap_or(&empty));
}
