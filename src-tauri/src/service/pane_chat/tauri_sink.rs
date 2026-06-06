//! Tauri-backed `DeltaSink`. Emits one event per delta, channel name
//! `pane-chat-{pane_id}` — per-pane subscription keeps the wire
//! traffic scoped (no global firehose; the frontend only listens to
//! the pane it's actually showing in UI mode).

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::agent::chat_types::RichMessage;
use crate::service::pane_chat::DeltaSink;

#[derive(Serialize)]
struct DeltaEvent<'a> {
    pane_id: &'a str,
    messages: &'a [RichMessage],
}

pub struct TauriDeltaSink {
    app: AppHandle,
}

impl TauriDeltaSink {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl DeltaSink for TauriDeltaSink {
    fn emit(&self, pane_id: &str, messages: Vec<RichMessage>) {
        let event = format!("pane-chat-{}", pane_id);
        let payload = DeltaEvent {
            pane_id,
            messages: &messages,
        };
        if let Err(e) = self.app.emit(&event, &payload) {
            log::warn!("[pane_chat] emit {} failed: {}", event, e);
        }
    }
}

/// Convenience constructor for the common case — wraps a fresh
/// [`TauriDeltaSink`] in `Arc<dyn DeltaSink>`.
pub fn make_sink(app: AppHandle) -> Arc<dyn DeltaSink> {
    Arc::new(TauriDeltaSink::new(app))
}
