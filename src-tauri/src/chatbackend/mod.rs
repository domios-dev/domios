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

use crate::agent::chat_types::StructuredChat;
use crate::agent::provider::{
    BehaviorConfig, ModelInfo, ProviderDescriptor, ProviderTransport, ProviderTrust,
    SystemPromptDelivery, PROVIDER_DESCRIPTOR_SCHEMA_VERSION,
};
use crate::plugin::capability::CapabilityRegistry;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct PresetInfo {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Live state of an open chatBackend session, surfaced to the pane's tab/bubble.
/// Currently just the model the session is talking to (a shell has no provider-
/// side store, so the plugin's own view is the truth). `model: None` ⇒ the
/// backend does not expose a session model (a bare backend with no `sessionInfo`).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ChatSessionInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
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

    /// Whether a user may open this backend as a pane from the new-session
    /// picker. Bare `chatBackend: true` ⇒ false (backend-only, e.g. an NPC
    /// driven by the host/agent); the manifest `{ spawnable: true }` form opts
    /// in. Default false (not user-spawnable) — core filters discovery and
    /// rejects spawn requests for non-spawnable backends on this signal.
    fn spawnable(&self) -> bool {
        false
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

    /// Pull transcript after `cursor`. The plugin export is
    /// `poll(sessionId, cursor) -> { messages, cursor }`: with a cursor it
    /// returns ONLY the delta since that bookmark plus the next cursor; with
    /// `None` it returns the whole transcript (and seeds the cursor). Empty
    /// `messages` ⇒ nothing new (the loop emits nothing and backs off). The
    /// plugin's `NormalizedChatMessage[]` deserializes directly into
    /// [`ChatMsg`](crate::agent::chat_types::ChatMsg) (`type` → `kind`).
    fn poll(
        &self,
        session_id: &str,
        cursor: Option<&str>,
    ) -> Result<StructuredChat, ChatBackendError>;

    /// Tear down a session. Best-effort; called on pane close.
    fn close_session(&self, session_id: &str) -> Result<(), ChatBackendError>;

    /// Models this backend offers, for the pane's model picker.
    fn list_models(&self) -> Result<Vec<ModelInfo>, ChatBackendError>;

    /// Presets this backend offers. Optional: backends without preset support
    /// return an empty list.
    fn list_presets(&self) -> Result<Vec<PresetInfo>, ChatBackendError> {
        Ok(Vec::new())
    }

    /// Current live state of an open session (the model it is talking to).
    /// Optional: a backend that does not expose a per-session model returns the
    /// default (`model: None`). The plugin export is `sessionInfo(sessionId)`.
    fn session_info(&self, session_id: &str) -> Result<ChatSessionInfo, ChatBackendError> {
        let _ = session_id;
        Ok(ChatSessionInfo::default())
    }

    /// Switch the model an open session uses on its next turn. Optional — a
    /// backend that cannot switch returns an error (the default), which the
    /// command surfaces. The plugin export is `setModel(sessionId, model)`.
    fn set_model(&self, session_id: &str, model: &str) -> Result<(), ChatBackendError> {
        let _ = (session_id, model);
        Err(ChatBackendError::Call(
            "this chatBackend does not support switching the model".into(),
        ))
    }

    /// Build the unified wire descriptor for this chat backend. Transport is
    /// always `ChatBackend`; models/presets come from the live getters.
    fn describe(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            schema_version: PROVIDER_DESCRIPTOR_SCHEMA_VERSION,
            id: self.id().to_string(),
            display_name: self.display_name().to_string(),
            icon_hint: self.icon_hint().map(|s| s.to_string()),
            icon_svg: self.icon_svg().map(|s| s.to_string()),
            icon_color: self.icon_color().map(|s| s.to_string()),
            available_models: self.list_models().unwrap_or_default(),
            default_model: None,
            settings_schema: Vec::new(),
            notifications_schema: Vec::new(),
            supports_lazy_skills: false,
            has_hooks: false,
            preserves_context_on_resume: true,
            cli_binary: None,
            install_command: None,
            model_switch_format: None,
            supports_stop: false,
            supports_chat_ui: true,
            trust: ProviderTrust::Unknown,
            transport: ProviderTransport::ChatBackend,
            behavior_config: BehaviorConfig::PresetsAndSystemPrompt,
            presets: self.list_presets().unwrap_or_default(),
            system_prompt_delivery: SystemPromptDelivery::SessionConfig,
        }
    }
}

pub type ChatBackendRegistry = CapabilityRegistry<dyn ChatBackendProvider>;
pub type ChatBackendRegistryHandle = Arc<ChatBackendRegistry>;

#[cfg(test)]
mod tests;
