//! Tauri sink for raw JSONL text deltas. Emits `pane-chat-raw-{id}`
//! events; the frontend parses with the TS-side `parseJsonlSession`.

use std::sync::Arc;

use tauri::{AppHandle, Emitter};

use crate::service::pane_chat::tailer::RawTextSink;

#[derive(serde::Serialize)]
struct RawDeltaEvent<'a> {
    pane_id: &'a str,
    text: &'a str,
}

pub struct TauriRawSink {
    app: AppHandle,
}

impl RawTextSink for TauriRawSink {
    fn emit(&self, pane_id: &str, text: String) {
        let event = format!("pane-chat-raw-{}", pane_id);
        let payload = RawDeltaEvent { pane_id, text: &text };
        if let Err(e) = self.app.emit(&event, &payload) {
            log::warn!("[pane_chat.raw] emit {} failed: {}", event, e);
        }
    }
}

pub fn make_sink(app: AppHandle) -> Arc<dyn RawTextSink> {
    Arc::new(TauriRawSink { app })
}
