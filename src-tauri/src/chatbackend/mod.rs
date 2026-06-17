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

    /// Open a session for a pane with the plugin's config. Returns the
    /// plugin-chosen session id (echoed back on every later call).
    fn open_session(&self, config: serde_json::Value) -> Result<String, ChatBackendError>;

    /// Deliver one user turn to the session.
    fn send_message(&self, session_id: &str, text: &str) -> Result<(), ChatBackendError>;

    /// Pull any new transcript messages. Empty ⇒ nothing new. The plugin's
    /// `NormalizedChatMessage[]` deserializes directly into [`ChatMsg`]
    /// (`type` → `kind`).
    fn poll(&self, session_id: &str) -> Result<Vec<ChatMsg>, ChatBackendError>;

    /// Tear down a session. Best-effort; called on pane close.
    fn close_session(&self, session_id: &str) -> Result<(), ChatBackendError>;

    /// Models this backend offers, for the pane's model picker.
    fn list_models(&self) -> Result<Vec<ModelInfo>, ChatBackendError>;
}

pub type ChatBackendRegistry = CapabilityRegistry<dyn ChatBackendProvider>;
pub type ChatBackendRegistryHandle = Arc<ChatBackendRegistry>;

#[cfg(test)]
mod tests;
