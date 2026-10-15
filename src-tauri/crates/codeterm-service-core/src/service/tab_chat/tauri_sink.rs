//! Tauri-backed `DeltaSink`. Emits one event per delta, channel name
//! `tab-chat-{tab_id}` — per-tab subscription keeps the wire
//! traffic scoped (no global firehose; the frontend only listens to
//! the tab it's actually showing in UI mode).

use std::sync::Arc;

use crate::ports::host::UiEvents;
use serde::Serialize;

use crate::agent::chat_types::RichMessage;
use crate::service::tab_chat::DeltaSink;

#[derive(Serialize)]
struct DeltaEvent<'a> {
    tab_id: &'a str,
    messages: &'a [RichMessage],
    start_offset: u64,
    offset: u64,
}

pub struct TauriDeltaSink {
    ui: Arc<dyn UiEvents>,
}

impl TauriDeltaSink {
    pub fn new(ui: Arc<dyn UiEvents>) -> Self {
        Self { ui }
    }
}

impl DeltaSink for TauriDeltaSink {
    fn emit(&self, tab_id: &str, messages: Vec<RichMessage>, start_offset: u64, offset: u64) {
        let event = format!("tab-chat-{}", tab_id);
        let payload = DeltaEvent {
            tab_id,
            messages: &messages,
            start_offset,
            offset,
        };
        self.ui.emit(
            &event,
            serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null),
        );
    }
}

/// Convenience constructor for the common case — wraps a fresh
/// [`TauriDeltaSink`] in `Arc<dyn DeltaSink>`.
pub fn make_sink(ui: Arc<dyn UiEvents>) -> Arc<dyn DeltaSink> {
    Arc::new(TauriDeltaSink::new(ui))
}
