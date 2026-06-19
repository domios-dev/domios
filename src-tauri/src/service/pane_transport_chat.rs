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
use crate::agent::chat_types::StructuredChat;
use crate::agent::provider::ModelInfo;
use crate::chatbackend::ChatBackendProvider;

/// Whether outbound preprocessor stages (`chatPreprocessor`) should run for a
/// chatBackend send. Mirrors the PTY gate ([`is_user_turn_send`]): only agent
/// panes get a `<context>` block injected. A plain shell chatBackend owns its
/// own prompt, so a registered preprocessor must NOT rewrite every shell send.
///
/// [`is_user_turn_send`]: crate::chatpipeline::is_user_turn_send
fn chatbackend_should_stage(pane_has_agent: bool) -> bool {
    pane_has_agent
}

fn build_chatbackend_open_session_ctx(
    pane_id: &str,
    config: serde_json::Value,
) -> serde_json::Value {
    let mut ctx = serde_json::Map::new();
    ctx.insert(
        "paneId".into(),
        serde_json::Value::String(pane_id.to_string()),
    );
    for key in ["systemPrompt", "preset", "model"] {
        if let Some(value) = config.get(key).and_then(|v| v.as_str()) {
            ctx.insert(key.into(), serde_json::Value::String(value.to_string()));
        }
    }
    ctx.insert("config".into(), config);
    serde_json::Value::Object(ctx)
}

pub struct ChatBackendTransport<'a> {
    pub(crate) svc: &'a ServiceContext,
    pub(crate) pane_id: String,
    pub(crate) plugin_id: String,
    pub(crate) session_id: String,
}

impl PaneTransport for ChatBackendTransport<'_> {
    fn send(&self, text: &str, _enter: bool) -> Result<(), String> {
        // R3 restore-gap: a pane restored after a daemon restart has no live
        // plugin session. Lazily re-open it (re-seeding from the persisted
        // systemPrompt/preset) before the first send so the conversation
        // continues. No-op for a normally-spawned pane.
        let session_id = self
            .svc
            .chatbackend_reopen_if_restored(&self.pane_id)
            .unwrap_or(None)
            .unwrap_or_else(|| self.session_id.clone());
        // Gate the preprocessor on agent-ness, like the PTY path — a plain shell
        // chatBackend must not get context injected into every send.
        let staged = if chatbackend_should_stage(self.svc.pane_has_agent(&self.pane_id)) {
            self.svc
                .run_outbound_stages(&self.pane_id, text.to_string())
        } else {
            text.to_string()
        };
        self.svc
            .chatbackend_send(&self.plugin_id, &session_id, &staged)
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
        // No cursor ⇒ the plugin returns the full transcript for the text read.
        let msgs = self
            .svc
            .chatbackend_poll(&self.plugin_id, &self.session_id, None)?
            .messages;
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
    /// envelope `{ paneId, config, systemPrompt?, preset?, model? }` and
    /// returns the plugin-chosen session id.
    pub fn chatbackend_open_session(
        &self,
        plugin_id: &str,
        pane_id: &str,
        config: serde_json::Value,
    ) -> Result<String, String> {
        let ctx = build_chatbackend_open_session_ctx(pane_id, config);
        self.chatbackend_provider(plugin_id)?
            .open_session(ctx)
            .map_err(|e| e.to_string())
    }

    /// Lazily re-open a chatBackend pane's plugin session after a daemon
    /// restart. A restored pane carries `backing: ChatBackend { .. }` but the
    /// plugin VM has no live session for the persisted id. On the first send we
    /// re-open from the persisted openSession config (the JSONL `_meta` header),
    /// migrate history to the (possibly new) session id so it stays continuous,
    /// update the pane backing, and start the inbound poll loop.
    ///
    /// Returns `Some(effective_session_id)` if a re-open happened, `None` if the
    /// pane was already live (a no-op). Best-effort: a missing meta/plugin is a
    /// soft error so a plain shell still sends.
    pub fn chatbackend_reopen_if_restored(&self, pane_id: &str) -> Result<Option<String>, String> {
        use crate::multiplexer::types::PaneBacking;

        let (plugin_id, old_session) = {
            let mux = self.mux.lock();
            let Some(resolved) = mux.resolve_pane_id(pane_id) else {
                return Ok(None);
            };
            let Some(pane) = mux.get_pane(&resolved) else {
                return Ok(None);
            };
            match (&pane.backing, pane.restored_from_snapshot) {
                (
                    PaneBacking::ChatBackend {
                        plugin_id,
                        session_id,
                    },
                    true,
                ) => (plugin_id.clone(), session_id.clone()),
                _ => return Ok(None),
            }
        };

        let path =
            crate::service::pane_chat::chatbackend_history::session_path(&plugin_id, &old_session);
        let config = crate::service::pane_chat::chatbackend_history::read_meta(&path)
            .unwrap_or(serde_json::Value::Null);

        let new_session = self.chatbackend_open_session(&plugin_id, pane_id, config.clone())?;
        crate::service::pane_chat::chatbackend_history::migrate(
            &plugin_id,
            &old_session,
            &new_session,
        );

        {
            let mut mux = self.mux.lock();
            if let Some(resolved) = mux.resolve_pane_id(pane_id) {
                if let Some(pane) = mux.get_pane_mut(&resolved) {
                    pane.backing = PaneBacking::ChatBackend {
                        plugin_id: plugin_id.clone(),
                        session_id: new_session.clone(),
                    };
                    pane.restored_from_snapshot = false;
                }
            }
        }

        self.spawn_chatbackend_poll_loop(
            pane_id.to_string(),
            plugin_id,
            new_session.clone(),
            config,
        );
        Ok(Some(new_session))
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

    /// Deliver one user turn to a chatBackend pane, re-opening a restored
    /// pane's plugin session first. After a daemon restart a pane carries
    /// `backing: ChatBackend { .. }` but the plugin VM has no live session and
    /// no poll loop — so the raw [`chatbackend_send`] would hit a dead session
    /// and never stream a reply. [`chatbackend_reopen_if_restored`] rewrites the
    /// backing to the live session id and starts the poll loop; we then read the
    /// (possibly new) backing and send. A no-op re-open for a live pane.
    pub(crate) fn chatbackend_pane_send(&self, pane_id: &str, text: &str) -> Result<(), String> {
        self.chatbackend_reopen_if_restored(pane_id)?;
        let (plugin_id, session_id) = self
            .pane_chat_backing(pane_id)
            .ok_or("chatBackend pane lost its backing")?;
        let staged = self.run_outbound_stages(pane_id, text.to_string());
        self.chatbackend_send(&plugin_id, &session_id, &staged)
    }

    /// Advance an async chatBackend's in-flight work for a session (Item 6).
    /// Called each poll-loop tick before `chatbackend_poll`; best-effort.
    pub fn chatbackend_pump(&self, plugin_id: &str, session_id: &str) -> Result<(), String> {
        self.chatbackend_provider(plugin_id)?
            .pump(session_id)
            .map_err(|e| e.to_string())
    }

    /// Pull the transcript delta after `cursor` from a plugin session (no
    /// cursor ⇒ the full transcript). Returns `{ messages, cursor }`.
    pub fn chatbackend_poll(
        &self,
        plugin_id: &str,
        session_id: &str,
        cursor: Option<&str>,
    ) -> Result<StructuredChat, String> {
        self.chatbackend_provider(plugin_id)?
            .poll(session_id, cursor)
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

    pub fn chatbackend_list_presets(
        &self,
        plugin_id: &str,
    ) -> Result<Vec<crate::chatbackend::PresetInfo>, String> {
        self.chatbackend_provider(plugin_id)?
            .list_presets()
            .map_err(|e| e.to_string())
    }

    /// Current live state of an open chatBackend session (the model it talks to).
    pub fn chatbackend_session_info(
        &self,
        plugin_id: &str,
        session_id: &str,
    ) -> Result<crate::chatbackend::ChatSessionInfo, String> {
        self.chatbackend_provider(plugin_id)?
            .session_info(session_id)
            .map_err(|e| e.to_string())
    }

    /// Switch the model an open chatBackend session uses on its next turn, then
    /// return the refreshed session info so the caller can reflect it immediately.
    pub fn chatbackend_set_model(
        &self,
        plugin_id: &str,
        session_id: &str,
        model: &str,
    ) -> Result<crate::chatbackend::ChatSessionInfo, String> {
        let provider = self.chatbackend_provider(plugin_id)?;
        provider
            .set_model(session_id, model)
            .map_err(|e| e.to_string())?;
        provider.session_info(session_id).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn chatbackend_send_is_gated_on_agent_pane() {
        // M3: a plain shell chatBackend (no detected agent_type) must NOT run the
        // outbound preprocessor — shells own their prompt. Only an agent pane gets
        // context injected, matching the PTY `is_user_turn_send` gate.
        assert!(
            !super::chatbackend_should_stage(false),
            "shell chatBackend send must skip outbound stages"
        );
        assert!(
            super::chatbackend_should_stage(true),
            "agent chatBackend send still runs outbound stages"
        );
    }

    #[test]
    fn chatbackend_open_session_ctx_promotes_open_shell_fields() {
        let ctx = super::build_chatbackend_open_session_ctx(
            "pane-7",
            json!({
                "systemPrompt": "You are CodeTerm.",
                "preset": "codeterm",
                "model": "qwen",
                "temperature": 0.2
            }),
        );

        assert_eq!(ctx["paneId"], "pane-7");
        assert_eq!(ctx["config"]["temperature"], 0.2);
        assert_eq!(ctx["systemPrompt"], "You are CodeTerm.");
        assert_eq!(ctx["preset"], "codeterm");
        assert_eq!(ctx["model"], "qwen");
    }

    #[test]
    fn chatbackend_open_session_ctx_omits_absent_open_shell_fields() {
        let ctx = super::build_chatbackend_open_session_ctx("pane-7", json!({}));

        assert_eq!(ctx["paneId"], "pane-7");
        assert_eq!(ctx["config"], json!({}));
        assert!(ctx.get("systemPrompt").is_none());
        assert!(ctx.get("preset").is_none());
        assert!(ctx.get("model").is_none());
    }
}
