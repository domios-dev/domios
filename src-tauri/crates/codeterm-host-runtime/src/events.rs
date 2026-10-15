use serde::Serialize;

pub fn emit_everywhere<R: tauri::Runtime, S: Serialize>(
    app: &tauri::AppHandle<R>,
    event_type: &str,
    data: S,
) {
    use tauri::{Emitter, Manager};
    let value = serde_json::to_value(&data).unwrap_or(serde_json::Value::Null);
    let _ = app.emit(event_type, &value);
    if let Some(events) = app.try_state::<codeterm_events::EventBroadcasterState>() {
        events.publish_generic(
            event_type,
            codeterm_events::web_safe_event_payload(event_type, value),
        );
    }
}

#[cfg(test)]
mod tests;
