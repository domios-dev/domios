//! `ChatBackendTransport` — the non-PTY [`PaneTransport`] impl (plan B1.3 #3).
//!
//! Routes pane I/O to the plugin `chatBackend` capability resolved from
//! `svc.chatbackend_registry`: `send` → `sendMessage`, reads → the polled
//! transcript. `resize` is a graceful no-op (a chat session has no terminal to
//! resize); `kill` closes the plugin session (`closeSession`). Pane teardown
//! (`close_pane`) closes the session too, so a normal pane-close also releases
//! the plugin's model/context.

use std::sync::Arc;

use super::pane_transport::PaneTransport;
use super::pty_io::SendBatchItem;
use super::ServiceContext;
use crate::agent::chat_types::ChatMsg;
use crate::agent::provider::ModelInfo;
use crate::chatbackend::ChatBackendProvider;

pub struct ChatBackendTransport<'a> {
    pub(crate) svc: &'a ServiceContext,
    pub(crate) pane_id: String,
    pub(crate) plugin_id: String,
    pub(crate) session_id: String,
}

impl PaneTransport for ChatBackendTransport<'_> {
    fn send(&self, text: &str, _enter: bool) -> Result<(), String> {
        let staged = self
            .svc
            .run_outbound_stages(&self.pane_id, text.to_string());
        self.svc
            .chatbackend_send(&self.plugin_id, &self.session_id, &staged)
    }

    fn send_batch(&self, batch: Vec<SendBatchItem>) -> Result<(), String> {
        for item in batch {
            self.send(&item.text, item.enter)?;
        }
        Ok(())
    }

    fn resize(&self, _cols: u16, _rows: u16) -> Result<(), String> {
        Ok(())
    }

    fn kill(&self) -> Result<(), String> {
        self.svc
            .chatbackend_close(&self.plugin_id, &self.session_id)
    }

    fn read_output(
        &self,
        _offset: Option<usize>,
        _max_bytes: Option<usize>,
        _strip_ansi: bool,
        _full: bool,
    ) -> Result<serde_json::Value, String> {
        let text = self.transcript_text()?;
        Ok(serde_json::json!({ "output": text, "offset": text.len() }))
    }

    fn read_screen(&self, formatted: bool) -> Result<serde_json::Value, String> {
        let text = self.transcript_text()?;
        Ok(serde_json::json!({ "text": text, "formatted": formatted }))
    }
}

impl ChatBackendTransport<'_> {
    /// Render the plugin transcript as plain text for the CLI/REST text reads.
    fn transcript_text(&self) -> Result<String, String> {
        let msgs = self
            .svc
            .chatbackend_poll(&self.plugin_id, &self.session_id)?;
        let text = msgs
            .into_iter()
            .map(|m| m.content)
            .collect::<Vec<_>>()
            .join("\n");
        Ok(text)
    }
}

impl ServiceContext {
    fn chatbackend_provider(
        &self,
        plugin_id: &str,
    ) -> Result<Arc<dyn ChatBackendProvider>, String> {
        self.chatbackend_registry
            .get(plugin_id)
            .ok_or_else(|| format!("chatBackend plugin '{plugin_id}' not registered"))
    }

    /// Open a plugin session for a pane. Builds the SDK `openSession` ctx
    /// envelope `{ paneId, config }` (the plugin may key its session on
    /// `paneId`, e.g. LM Studio) and returns the plugin-chosen session id.
    pub fn chatbackend_open_session(
        &self,
        plugin_id: &str,
        pane_id: &str,
        config: serde_json::Value,
    ) -> Result<String, String> {
        let ctx = serde_json::json!({ "paneId": pane_id, "config": config });
        self.chatbackend_provider(plugin_id)?
            .open_session(ctx)
            .map_err(|e| e.to_string())
    }

    /// Deliver one user turn to a plugin session.
    pub fn chatbackend_send(
        &self,
        plugin_id: &str,
        session_id: &str,
        text: &str,
    ) -> Result<(), String> {
        self.chatbackend_provider(plugin_id)?
            .send_message(session_id, text)
            .map_err(|e| e.to_string())
    }

    /// Advance an async chatBackend's in-flight work for a session (Item 6).
    /// Called each poll-loop tick before `chatbackend_poll`; best-effort.
    pub fn chatbackend_pump(&self, plugin_id: &str, session_id: &str) -> Result<(), String> {
        self.chatbackend_provider(plugin_id)?
            .pump(session_id)
            .map_err(|e| e.to_string())
    }

    /// Pull new transcript messages from a plugin session.
    pub fn chatbackend_poll(
        &self,
        plugin_id: &str,
        session_id: &str,
    ) -> Result<Vec<ChatMsg>, String> {
        self.chatbackend_provider(plugin_id)?
            .poll(session_id)
            .map_err(|e| e.to_string())
    }

    /// Tear down a plugin session (best-effort).
    pub fn chatbackend_close(&self, plugin_id: &str, session_id: &str) -> Result<(), String> {
        self.chatbackend_provider(plugin_id)?
            .close_session(session_id)
            .map_err(|e| e.to_string())
    }

    /// Models a chatBackend plugin offers, for the pane's model picker.
    pub fn chatbackend_list_models(&self, plugin_id: &str) -> Result<Vec<ModelInfo>, String> {
        self.chatbackend_provider(plugin_id)?
            .list_models()
            .map_err(|e| e.to_string())
    }
}
