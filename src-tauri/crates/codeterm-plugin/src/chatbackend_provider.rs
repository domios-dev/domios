use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use codeterm_types::agent::chat_types::{ChatMsg, StructuredChat};
use parking_lot::RwLock;

use crate::model_catalog::{self, RegisteredDefaults};
use crate::provider::{
    BehaviorConfig, ContextOwner, ModelInfo, ProviderAvailability, ProviderDescriptor,
    ProviderTransport, ProviderTrust, SystemPromptDelivery, PROVIDER_DESCRIPTOR_SCHEMA_VERSION,
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

/// One user entry plus everything the assistant produced in response to it, in
/// source order. The window's atomic unit: a tool call and its result are never
/// individually addressable, so a boundary cannot split the pair.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TurnGroup {
    pub entries: Vec<ChatMsg>,
}

/// One host-composed turn. `system` is a sibling field of `groups`, never an
/// element of it — no slice, filter, or index over the counted collection can
/// reach the role brief.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HostTurn {
    pub system: String,
    pub groups: Vec<TurnGroup>,
}

/// Refusal a backend that does not own host context returns from `send_turn`.
pub const CONTEXT_OWNER_UNSUPPORTED: &str = "this chatBackend does not own host context";

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

    /// Who composes this backend's context. `Plugin` (default) ⇒ the backend
    /// owns its own history, which is today's behaviour for every provider.
    fn context_owner(&self) -> ContextOwner {
        ContextOwner::Plugin
    }

    /// Whether this backend actually implements [`send_turn`](Self::send_turn).
    /// Declaring host ownership without it is refused at session open rather
    /// than downgraded to `send_message` at send time.
    fn supports_send_turn(&self) -> bool {
        false
    }

    /// Deliver one host-composed turn. Optional — a backend that does not own
    /// host context refuses, so it can never be silently routed into this path.
    fn send_turn(&self, session_id: &str, turn: HostTurn) -> Result<(), ChatBackendError> {
        let _ = (session_id, turn);
        Err(ChatBackendError::Call(CONTEXT_OWNER_UNSUPPORTED.into()))
    }

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
            model_groups: Vec::new(),
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
            badge: None,
            update_command: None,
            model_switch_format: None,
            fresh_context_command: None,
            compact_command: None,
            self_update: None,
            integration_trust_grant: None,
            supports_stop: false,
            supports_chat_ui: true,
            supports_structured_prompts: false,
            supports_permission_auto_confirm: false,
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
            context_owner: self.context_owner(),
            availability: ProviderAvailability::default(),
        }
    }
}

const MAX_CAPABILITY_ERROR_CHARS: usize = 256;

struct RegisteredChatBackend {
    provider: Arc<dyn ChatBackendProvider>,
    descriptor: ProviderDescriptor,
    model_error: Option<String>,
}

/// Lifecycle-owned chatBackend registry. Consumers read cached descriptor snapshots.
pub struct ChatBackendRegistry {
    entries: Arc<RwLock<HashMap<String, RegisteredChatBackend>>>,
    discovery_enabled: AtomicBool,
}

impl ChatBackendRegistry {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            discovery_enabled: AtomicBool::new(false),
        }
    }

    pub fn register(&self, id: impl Into<String>, provider: Arc<dyn ChatBackendProvider>) {
        let id = id.into();
        self.register_cached(id.clone(), provider.clone());
        if self.discovery_enabled.load(Ordering::Acquire) {
            let _ = self.refresh_background(id, provider);
        }
    }

    fn register_cached(&self, id: String, provider: Arc<dyn ChatBackendProvider>) {
        let mut descriptor = provider.describe();
        descriptor.id = id.clone();
        self.entries.write().insert(
            id,
            RegisteredChatBackend {
                provider,
                descriptor,
                model_error: None,
            },
        );
    }

    fn refresh_background(
        &self,
        id: String,
        provider: Arc<dyn ChatBackendProvider>,
    ) -> Option<std::thread::JoinHandle<()>> {
        let entries = self.entries.clone();
        let thread_id = id.clone();
        match std::thread::Builder::new()
            .name(format!("chatbackend-catalog-{id}"))
            .spawn(move || {
                let presets = provider.list_presets();
                let result = provider.list_models();
                Self::apply_capability_result(&entries, &thread_id, &provider, presets, result);
            }) {
            Ok(handle) => Some(handle),
            Err(error) => {
                log::warn!("[chatbackend.catalog] {id} refresh thread failed: {error}");
                None
            }
        }
    }

    #[doc(hidden)]
    pub fn register_and_wait(&self, id: impl Into<String>, provider: Arc<dyn ChatBackendProvider>) {
        let id = id.into();
        self.register_cached(id.clone(), provider.clone());
        if let Some(handle) = self.refresh_background(id, provider) {
            handle.join().unwrap();
        }
    }

    pub fn start_discovery(&self) -> Vec<std::thread::JoinHandle<()>> {
        if self.discovery_enabled.swap(true, Ordering::AcqRel) {
            return Vec::new();
        }
        let providers = self
            .entries
            .read()
            .iter()
            .map(|(id, entry)| (id.clone(), entry.provider.clone()))
            .collect::<Vec<_>>();
        providers
            .into_iter()
            .filter_map(|(id, provider)| self.refresh_background(id, provider))
            .collect()
    }

    fn apply_capability_result(
        entries: &RwLock<HashMap<String, RegisteredChatBackend>>,
        id: &str,
        provider: &Arc<dyn ChatBackendProvider>,
        presets: Result<Vec<PresetInfo>, ChatBackendError>,
        result: Result<Vec<ModelInfo>, ChatBackendError>,
    ) {
        let (models, model_error) = match result {
            Ok(models) => (normalize_models(models), None),
            Err(error) => {
                let error = bounded_capability_error(&error.to_string());
                log::warn!("[chatbackend.catalog] {id} listModels failed: {error}");
                (Vec::new(), Some(error))
            }
        };
        let mut entries = entries.write();
        let Some(entry) = entries.get_mut(id) else {
            return;
        };
        if !Arc::ptr_eq(&entry.provider, provider) {
            return;
        }
        entry.descriptor.presets = match presets {
            Ok(presets) => presets,
            Err(error) => {
                log::warn!(
                    "[chatbackend.catalog] {id} listPresets failed: {}",
                    bounded_capability_error(&error.to_string())
                );
                Vec::new()
            }
        };
        model_catalog::replace_resolved(
            id,
            RegisteredDefaults {
                models,
                default: None,
            },
        );
        let catalog = model_catalog::registered_snapshot(id);
        entry.descriptor.available_models = catalog.models;
        entry.descriptor.default_model = catalog.default;
        entry.model_error = model_error;
    }

    pub fn unregister(&self, id: &str) {
        if self.entries.write().remove(id).is_some() {
            model_catalog::unregister(id);
        }
    }

    /// Drop the entry but keep its models. A reload re-registers immediately and
    /// resolves the new models asynchronously; clearing the catalog first leaves
    /// a window where the plugin is registered and answers with none.
    pub fn unregister_for_reload(&self, id: &str) {
        self.entries.write().remove(id);
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
    use std::sync::{mpsc, Mutex};

    use super::*;

    struct ListingBackend {
        id: &'static str,
        calls: Arc<CatalogCallTracker>,
        models: Result<Vec<ModelInfo>, String>,
    }

    #[derive(Default)]
    struct CatalogCallTracker {
        calls: Mutex<usize>,
    }

    impl CatalogCallTracker {
        fn record(&self) {
            *self.calls.lock().unwrap() += 1;
        }

        fn count(&self) -> usize {
            *self.calls.lock().unwrap()
        }
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
            self.calls.record();
            self.models.clone().map_err(ChatBackendError::Call)
        }
    }

    fn model(id: &str, display_name: &str, available: Option<bool>) -> ModelInfo {
        ModelInfo {
            id: id.into(),
            display_name: display_name.into(),
            description: Some(format!("{id} description")),
            available,
            availability: None,
            featured: None,
            badge: None,
        }
    }

    struct BlockingBackend {
        started: mpsc::Sender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }

    impl ChatBackendProvider for BlockingBackend {
        fn id(&self) -> &str {
            "chatbackend-blocking-fixture"
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
            self.started.send(()).unwrap();
            self.release.lock().unwrap().recv().unwrap();
            Ok(vec![model("ready-model", "Ready model", None)])
        }
    }

    #[test]
    fn registration_does_not_wait_for_live_model_discovery() {
        let registry = ChatBackendRegistry::new();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        registry.register(
            "chatbackend-blocking-fixture",
            Arc::new(BlockingBackend {
                started: started_tx,
                release: Mutex::new(release_rx),
            }),
        );
        assert!(registry
            .descriptor("chatbackend-blocking-fixture")
            .is_some());
        assert!(registry
            .descriptor("chatbackend-blocking-fixture")
            .unwrap()
            .available_models
            .is_empty());

        let handles = registry.start_discovery();
        started_rx.recv().unwrap();
        release_tx.send(()).unwrap();
        handles
            .into_iter()
            .for_each(|handle| handle.join().unwrap());
        assert_eq!(
            registry
                .descriptor("chatbackend-blocking-fixture")
                .unwrap()
                .available_models[0]
                .id,
            "ready-model"
        );
        registry.unregister("chatbackend-blocking-fixture");
    }

    #[test]
    fn chatbackend_list_models_populates_descriptor_catalog() {
        let registry = ChatBackendRegistry::new();
        let calls = Arc::new(CatalogCallTracker::default());
        registry.register_and_wait(
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
        assert_eq!(calls.count(), 1);
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
            calls.count(),
            1,
            "describe reads the central snapshot without calling list_models"
        );
        registry.unregister("chatbackend-populates-fixture");
    }

    #[test]
    fn failed_refresh_clears_only_that_provider_and_bounds_the_error() {
        let registry = ChatBackendRegistry::new();
        registry.register_and_wait(
            "chatbackend-error-fixture",
            Arc::new(ListingBackend {
                id: "chatbackend-error-fixture",
                calls: Arc::new(CatalogCallTracker::default()),
                models: Ok(vec![model("sentinel-a", "Sentinel A", None)]),
            }),
        );
        registry.register_and_wait("foreign-chatbackend-fixture", Arc::new(ForeignBackend));

        registry.register_and_wait(
            "chatbackend-error-fixture",
            Arc::new(ListingBackend {
                id: "chatbackend-error-fixture",
                calls: Arc::new(CatalogCallTracker::default()),
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
        let calls = Arc::new(CatalogCallTracker::default());
        registry.register_and_wait(
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
        assert_eq!(calls.count(), 1);

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
