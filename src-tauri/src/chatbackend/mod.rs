//! ChatBackend capability — a plugin provides a full conversational backend
//! (e.g. LM Studio, an HTTP LLM) that drives a non-PTY pane. The pane's I/O is
//! routed here by the [`PaneTransport`](crate::service::pane_transport) seam:
//! `send` → `sendMessage`, reads → the polled transcript. There is no native
//! chatBackend — the capability is JS-only (plan §1.10), so the acceptance
//! oracle is a `call_json` contract test (see `plugin::js_runtime::chatbackend`),
//! not `parity_test.rs`.
//!
//! Core asks the capability, never the identity — resolution goes through
//! [`ChatBackendRegistry`] (a
//! [`CapabilityRegistry`](crate::plugin::capability::CapabilityRegistry)).

use std::sync::Arc;

use crate::agent::chat_types::ChatMsg;
use crate::agent::provider::ModelInfo;
use crate::plugin::capability::CapabilityRegistry;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct PresetInfo {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug)]
pub enum ChatBackendError {
    /// The plugin's exported function threw, returned an error field, or the
    /// adapter could not coerce its output.
    Call(String),
}

impl std::fmt::Display for ChatBackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Call(e) => write!(f, "chatBackend call failed: {e}"),
        }
    }
}

impl std::error::Error for ChatBackendError {}

/// A plugin-contributed conversational backend. Methods are synchronous because
/// the QuickJS `call_json` bridge is synchronous (the VM is not `Send`); the
/// inbound poll loop calls `poll` from a background task.
pub trait ChatBackendProvider: Send + Sync {
    fn id(&self) -> &str;

    fn display_name(&self) -> &str {
        self.id()
    }

    fn icon_hint(&self) -> Option<&str> {
        None
    }

    fn icon_svg(&self) -> Option<&str> {
        None
    }

    fn icon_color(&self) -> Option<&str> {
        None
    }

    /// Open a session for a pane. `ctx` is the SDK envelope
    /// `{ paneId, config, systemPrompt?, preset?, model? }` (a plugin may key
    /// its session on `paneId`, e.g. LM Studio). Returns the plugin-chosen
    /// session id (echoed back on every later call).
    fn open_session(&self, ctx: serde_json::Value) -> Result<String, ChatBackendError>;

    /// Deliver one user turn to the session.
    fn send_message(&self, session_id: &str, text: &str) -> Result<(), ChatBackendError>;

    /// Advance any in-flight async work for the session, called by the host poll
    /// loop each tick BEFORE [`poll`](Self::poll). An async chatBackend (e.g. the
    /// dread NPC plugin) does its turn-advance here: check `host.agent` tickets,
    /// move completed replies into the transcript `poll` then surfaces. Without a
    /// host-driven pump such a plugin would never advance on the real daemon (the
    /// mock harness called `pump()` itself; the host never did) — Item 6.
    ///
    /// Optional: the default is a no-op, and a plugin with no `pump` export is
    /// unaffected. Best-effort — a pump error must not kill the poll loop.
    fn pump(&self, session_id: &str) -> Result<(), ChatBackendError> {
        let _ = session_id;
        Ok(())
    }

    /// Pull any new transcript messages. Empty ⇒ nothing new. The plugin's
    /// `NormalizedChatMessage[]` deserializes directly into [`ChatMsg`]
    /// (`type` → `kind`).
    fn poll(&self, session_id: &str) -> Result<Vec<ChatMsg>, ChatBackendError>;

    /// Tear down a session. Best-effort; called on pane close.
    fn close_session(&self, session_id: &str) -> Result<(), ChatBackendError>;

    /// Models this backend offers, for the pane's model picker.
    fn list_models(&self) -> Result<Vec<ModelInfo>, ChatBackendError>;

    /// Presets this backend offers. Optional: backends without preset support
    /// return an empty list.
    fn list_presets(&self) -> Result<Vec<PresetInfo>, ChatBackendError> {
        Ok(Vec::new())
    }
}

pub type ChatBackendRegistry = CapabilityRegistry<dyn ChatBackendProvider>;
pub type ChatBackendRegistryHandle = Arc<ChatBackendRegistry>;

#[cfg(test)]
mod tests;
