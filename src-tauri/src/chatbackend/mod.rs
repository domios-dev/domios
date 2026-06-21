//! ChatBackend capability — a plugin drives a non-PTY pane as a full
//! conversational backend, routed here by the [`PaneTransport`](crate::service::pane_transport)
//! seam. JS-only (no native impl), so the acceptance oracle is a `call_json`
//! contract test, not `parity_test.rs`. Core asks the capability via
//! [`ChatBackendRegistry`], never the identity.

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

/// Live state of an open chatBackend session — currently just its model. `model: None` ⇒ the backend exposes no session model.
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

    /// Whether a user may open this backend as a pane. Bare `chatBackend: true` ⇒
    /// false (backend-only); `{ spawnable: true }` opts in. Core gates discovery/spawn on this.
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

    /// Open a session for a pane (`ctx` = `{ paneId, config, systemPrompt?, preset?, model? }`); returns the plugin-chosen session id.
    fn open_session(&self, ctx: serde_json::Value) -> Result<String, ChatBackendError>;

    /// Deliver one user turn to the session.
    fn send_message(&self, session_id: &str, text: &str) -> Result<(), ChatBackendError>;

    /// Advance in-flight async work, called by the poll loop each tick before
    /// [`poll`](Self::poll) so an async chatBackend progresses on the real daemon.
    /// Optional no-op default; best-effort — a pump error must not kill the loop.
    fn pump(&self, session_id: &str) -> Result<(), ChatBackendError> {
        let _ = session_id;
        Ok(())
    }

    /// Pull transcript after `cursor`: with a cursor, only the delta + next cursor;
    /// with `None`, the whole transcript (seeding the cursor). Empty `messages` ⇒ nothing new.
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

    /// Current live state of an open session. Optional: a backend with no per-session model returns the default.
    fn session_info(&self, session_id: &str) -> Result<ChatSessionInfo, ChatBackendError> {
        let _ = session_id;
        Ok(ChatSessionInfo::default())
    }

    /// Switch the model an open session uses next turn. Optional — a backend that can't switch returns an error (default).
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
            supports_structured_prompts: false,
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
