use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use codeterm_types::agent::chat_types::StructuredChat;
use parking_lot::RwLock;

use crate::model_catalog::{self, RegisteredDefaults};
use crate::provider::{
    BehaviorConfig, ModelInfo, ProviderDescriptor, ProviderTransport, ProviderTrust,
    SystemPromptDelivery, PROVIDER_DESCRIPTOR_SCHEMA_VERSION,
};

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

    /// Whether a user may open this backend as a tab. Bare `chatBackend: true` ⇒
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

    /// Open a session for a tab (`ctx` = `{ tabId, config, systemPrompt?, preset?, model? }`); returns the plugin-chosen session id.
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

    /// Tear down a session. Best-effort; called on tab close.
    fn close_session(&self, session_id: &str) -> Result<(), ChatBackendError>;

    /// Models this backend offers, for the tab's model picker.
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

    fn watcher_capable(&self) -> bool {
        false
    }

    /// Whether this backend accepts host-composed instruction through the
    /// `openSession` systemPrompt context.
    fn supports_session_config(&self) -> bool {
        false
    }

    fn watcher_tick(
        &self,
        session_id: &str,
        input: serde_json::Value,
    ) -> Result<(), ChatBackendError> {
        let _ = (session_id, input);
        Err(ChatBackendError::Call(
            "this chatBackend does not support watcher mode".into(),
        ))
    }

    /// Build the unified wire descriptor for this chat backend. Transport is
    /// always `ChatBackend`. Keep this pure/cheap: provider descriptors are
    /// requested from UI hot paths, while live chatBackend getters enter the
    /// plugin VM and can block unrelated session loading.
    fn describe(&self) -> ProviderDescriptor {
        let catalog = model_catalog::registered_snapshot(self.id());
        ProviderDescriptor {
            schema_version: PROVIDER_DESCRIPTOR_SCHEMA_VERSION,
            id: self.id().to_string(),
            display_name: self.display_name().to_string(),
            icon_hint: self.icon_hint().map(|s| s.to_string()),
            icon_svg: self.icon_svg().map(|s| s.to_string()),
            icon_color: self.icon_color().map(|s| s.to_string()),
            available_models: catalog.models,
            default_model: catalog.default,
            reasoning_efforts: Vec::new(),
            default_reasoning_effort: None,
            settings_schema: Vec::new(),
            notifications_schema: Vec::new(),
            supports_lazy_skills: false,
            has_hooks: false,
            supports_permission_bypass: false,
            cli_binary: None,
            install_command: None,
            model_switch_format: None,
            fresh_context_command: None,
            compact_command: None,
            self_update: None,
            integration_trust_grant: None,
            supports_stop: false,
            supports_chat_ui: true,
            supports_structured_prompts: false,
            supports_watcher: self.watcher_capable(),
            trust: ProviderTrust::Unknown,
            transport: ProviderTransport::ChatBackend,
            behavior_config: BehaviorConfig::PresetsAndSystemPrompt,
            presets: Vec::new(),
            system_prompt_delivery: if self.supports_session_config() {
                SystemPromptDelivery::SessionConfig
            } else {
                SystemPromptDelivery::External
            },
        }
    }
}

const MAX_CAPABILITY_ERROR_CHARS: usize = 256;

struct RegisteredChatBackend {
    provider: Arc<dyn ChatBackendProvider>,
    descriptor: ProviderDescriptor,
    model_error: Option<String>,
}

/// Lifecycle-owned chatBackend registry. Live capability calls happen only in
/// `register`; consumers read the cached descriptor snapshot.
pub struct ChatBackendRegistry {
    entries: RwLock<HashMap<String, RegisteredChatBackend>>,
}

impl ChatBackendRegistry {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
        }
    }

    pub fn register(&self, id: impl Into<String>, provider: Arc<dyn ChatBackendProvider>) {
        let id = id.into();
        let (models, model_error) = match provider.list_models() {
            Ok(models) => (normalize_models(models), None),
            Err(error) => {
                let error = bounded_capability_error(&error.to_string());
                log::warn!("[chatbackend.catalog] {id} listModels failed: {error}");
                (Vec::new(), Some(error))
            }
        };
        model_catalog::replace_resolved(
            &id,
            RegisteredDefaults {
                models,
                default: None,
            },
        );

        let mut descriptor = provider.describe();
        descriptor.id = id.clone();
        descriptor.presets = match provider.list_presets() {
            Ok(presets) => presets,
            Err(error) => {
                log::warn!(
                    "[chatbackend.catalog] {id} listPresets failed: {}",
                    bounded_capability_error(&error.to_string())
                );
                Vec::new()
            }
        };
        self.entries.write().insert(
            id,
            RegisteredChatBackend {
                provider,
                descriptor,
                model_error,
            },
        );
    }

    pub fn unregister(&self, id: &str) {
        if self.entries.write().remove(id).is_some() {
            model_catalog::unregister(id);
        }
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn ChatBackendProvider>> {
        self.entries
            .read()
            .get(id)
            .map(|entry| entry.provider.clone())
    }

    pub fn descriptor(&self, id: &str) -> Option<ProviderDescriptor> {
        self.entries
            .read()
            .get(id)
            .map(|entry| entry.descriptor.clone())
    }

    pub fn model_capability_error(&self, id: &str) -> Option<String> {
        self.entries
            .read()
            .get(id)
            .and_then(|entry| entry.model_error.clone())
    }

    pub fn active(&self) -> Vec<Arc<dyn ChatBackendProvider>> {
        self.entries
            .read()
            .values()
            .map(|entry| entry.provider.clone())
            .collect()
    }

    pub fn ids(&self) -> Vec<String> {
        self.entries.read().keys().cloned().collect()
    }

    pub fn contains(&self, id: &str) -> bool {
        self.entries.read().contains_key(id)
    }
}

impl Default for ChatBackendRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn normalize_models(models: Vec<ModelInfo>) -> Vec<ModelInfo> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::with_capacity(models.len());
    for mut model in models {
        model.id = model.id.trim().to_string();
        if model.id.is_empty() || !seen.insert(model.id.to_lowercase()) {
            continue;
        }
        model.display_name = model.display_name.trim().to_string();
        if model.display_name.is_empty() {
            model.display_name = model.id.clone();
        }
        model.description = model
            .description
            .map(|description| description.trim().to_string())
            .filter(|description| !description.is_empty());
        normalized.push(model);
    }
    normalized
}

fn bounded_capability_error(error: &str) -> String {
    let mut chars = error.chars();
    let bounded: String = chars
        .by_ref()
        .take(MAX_CAPABILITY_ERROR_CHARS.saturating_sub(1))
        .collect();
    if chars.next().is_some() {
        format!("{bounded}…")
    } else {
        bounded
    }
}

pub type ChatBackendRegistryHandle = Arc<ChatBackendRegistry>;

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct ListingBackend {
        id: &'static str,
        calls: Arc<AtomicUsize>,
        models: Result<Vec<ModelInfo>, String>,
    }

    impl ChatBackendProvider for ListingBackend {
        fn id(&self) -> &str {
            self.id
        }

        fn display_name(&self) -> &str {
            "Descriptor fixture"
        }

        fn spawnable(&self) -> bool {
            true
        }

        fn open_session(&self, _: serde_json::Value) -> Result<String, ChatBackendError> {
            unreachable!("catalog fixture does not open sessions")
        }

        fn send_message(&self, _: &str, _: &str) -> Result<(), ChatBackendError> {
            unreachable!("catalog fixture does not send messages")
        }

        fn poll(&self, _: &str, _: Option<&str>) -> Result<StructuredChat, ChatBackendError> {
            unreachable!("catalog fixture does not poll sessions")
        }

        fn close_session(&self, _: &str) -> Result<(), ChatBackendError> {
            unreachable!("catalog fixture does not close sessions")
        }

        fn list_models(&self) -> Result<Vec<ModelInfo>, ChatBackendError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.models.clone().map_err(ChatBackendError::Call)
        }
    }

    fn model(id: &str, display_name: &str, available: Option<bool>) -> ModelInfo {
        ModelInfo {
            id: id.into(),
            display_name: display_name.into(),
            description: Some(format!("{id} description")),
            available,
        }
    }

    #[test]
    fn chatbackend_list_models_populates_descriptor_catalog() {
        let registry = ChatBackendRegistry::new();
        let calls = Arc::new(AtomicUsize::new(0));
        registry.register(
            "chatbackend-populates-fixture",
            Arc::new(ListingBackend {
                id: "chatbackend-populates-fixture",
                calls: calls.clone(),
                models: Ok(vec![
                    model(" sentinel-a ", " Sentinel A ", Some(true)),
                    model("sentinel-b", "Sentinel B", Some(false)),
                ]),
            }),
        );

        let descriptor = registry
            .descriptor("chatbackend-populates-fixture")
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            descriptor
                .available_models
                .iter()
                .map(|model| (
                    model.id.as_str(),
                    model.display_name.as_str(),
                    model.available
                ))
                .collect::<Vec<_>>(),
            vec![
                ("sentinel-a", "Sentinel A", Some(true)),
                ("sentinel-b", "Sentinel B", Some(false)),
            ]
        );
        assert_eq!(descriptor.default_model, None);
        assert_eq!(
            registry
                .get("chatbackend-populates-fixture")
                .unwrap()
                .describe()
                .available_models,
            descriptor.available_models
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "describe reads the central snapshot without calling list_models"
        );
        registry.unregister("chatbackend-populates-fixture");
    }

    #[test]
    fn failed_refresh_clears_only_that_provider_and_bounds_the_error() {
        let registry = ChatBackendRegistry::new();
        registry.register(
            "chatbackend-error-fixture",
            Arc::new(ListingBackend {
                id: "chatbackend-error-fixture",
                calls: Arc::new(AtomicUsize::new(0)),
                models: Ok(vec![model("sentinel-a", "Sentinel A", None)]),
            }),
        );
        registry.register("foreign-chatbackend-fixture", Arc::new(ForeignBackend));

        registry.register(
            "chatbackend-error-fixture",
            Arc::new(ListingBackend {
                id: "chatbackend-error-fixture",
                calls: Arc::new(AtomicUsize::new(0)),
                models: Err("capability failed ".repeat(40)),
            }),
        );

        assert!(registry
            .descriptor("chatbackend-error-fixture")
            .unwrap()
            .available_models
            .is_empty());
        assert!(registry
            .model_capability_error("chatbackend-error-fixture")
            .is_some_and(|error| error.chars().count() <= MAX_CAPABILITY_ERROR_CHARS));
        assert_eq!(
            registry
                .descriptor("foreign-chatbackend-fixture")
                .unwrap()
                .available_models[0]
                .id,
            "foreign-model"
        );
        registry.unregister("chatbackend-error-fixture");
        registry.unregister("foreign-chatbackend-fixture");
    }

    #[test]
    fn registration_resolves_catalog_override_once_before_caching() {
        let id = "chatbackend-override-fixture";
        let override_path = model_catalog::models_dir().join(format!("{id}.toml"));
        std::fs::write(
            &override_path,
            r#"replace = true
default = "override-model"

[[models]]
id = "override-model"
display_name = "Override model"
description = "Explicit user override"
available = false
"#,
        )
        .unwrap();
        let registry = ChatBackendRegistry::new();
        let calls = Arc::new(AtomicUsize::new(0));
        registry.register(
            id,
            Arc::new(ListingBackend {
                id,
                calls: calls.clone(),
                models: Ok(vec![model("live-model", "Live model", Some(true))]),
            }),
        );

        let descriptor = registry.descriptor(id).unwrap();
        assert_eq!(descriptor.available_models.len(), 1);
        assert_eq!(descriptor.available_models[0].id, "override-model");
        assert_eq!(descriptor.available_models[0].available, Some(false));
        assert_eq!(descriptor.default_model.as_deref(), Some("override-model"));

        std::fs::write(
            &override_path,
            r#"replace = true

[[models]]
id = "late-model"
display_name = "Late model"
"#,
        )
        .unwrap();
        assert_eq!(
            registry.descriptor(id).unwrap().available_models[0].id,
            "override-model",
            "descriptor stays on the lifecycle snapshot until refresh"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        registry.unregister(id);
        let _ = std::fs::remove_file(override_path);
    }

    struct ForeignBackend;

    impl ChatBackendProvider for ForeignBackend {
        fn id(&self) -> &str {
            "foreign-chatbackend-fixture"
        }

        fn open_session(&self, _: serde_json::Value) -> Result<String, ChatBackendError> {
            unreachable!("catalog fixture does not open sessions")
        }

        fn send_message(&self, _: &str, _: &str) -> Result<(), ChatBackendError> {
            unreachable!("catalog fixture does not send messages")
        }

        fn poll(&self, _: &str, _: Option<&str>) -> Result<StructuredChat, ChatBackendError> {
            unreachable!("catalog fixture does not poll sessions")
        }

        fn close_session(&self, _: &str) -> Result<(), ChatBackendError> {
            unreachable!("catalog fixture does not close sessions")
        }

        fn list_models(&self) -> Result<Vec<ModelInfo>, ChatBackendError> {
            Ok(vec![model("foreign-model", "Foreign model", None)])
        }
    }
}
