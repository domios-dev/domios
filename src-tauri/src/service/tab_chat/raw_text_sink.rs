//! Tauri sink for raw JSONL text deltas. Emits `tab-chat-raw-{id}`
//! events; the frontend parses with the TS-side `parseJsonlSession`.
//! Mirrored to the SSE bus so the web mirror's chat view streams too.

use std::sync::Arc;

use tauri::AppHandle;

use crate::service::tab_chat::tailer::RawTextSink;

#[derive(serde::Serialize)]
struct RawDeltaEvent<'a> {
    tab_id: &'a str,
    text: &'a str,
}

pub struct TauriRawSink {
    app: AppHandle,
}

impl RawTextSink for TauriRawSink {
    fn emit(&self, tab_id: &str, text: String) {
        let event = format!("tab-chat-raw-{}", tab_id);
        let payload = RawDeltaEvent {
            tab_id,
            text: &text,
        };
        crate::api::events::emit_everywhere(
            &self.app,
            &event,
            serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null),
        );
    }
}

pub fn make_sink(app: AppHandle) -> Arc<dyn RawTextSink> {
    Arc::new(TauriRawSink { app })
}
