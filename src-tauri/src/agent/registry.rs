use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex, RwLock};

use super::provider::{AgentProvider, SessionMetadata};

pub use super::registry_search::{
    SessionSearchHit, SessionSearchOpts, SessionSnippet, DEFAULT_SESSION_FILE_BYTE_CAP,
};

pub struct AgentRegistry {
    providers: RwLock<HashMap<String, Arc<dyn AgentProvider>>>,
    /// First provider displaced for an id (the native one a JS plugin took
    /// authority over). Kept so a JS plugin can delegate capability methods it
    /// doesn't implement to the native impl. Never overwritten by later
    /// re-registrations (hot-reload), so the fallback survives plugin reloads.
    native_fallbacks: RwLock<HashMap<String, Arc<dyn AgentProvider>>>,
    startup_loading: Mutex<StartupProviderLoading>,
    startup_loading_changed: Condvar,
}

#[derive(Default)]
struct StartupProviderLoading {
    pending: HashSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderStartupStatus {
    Ready,
    LoadingTimedOut,
    Unknown,
}

pub const PROVIDER_STARTUP_WAIT: Duration = Duration::from_secs(5);

impl AgentRegistry {
    /// Create an empty registry. Providers are added via `register_provider()`.
    pub fn new() -> Self {
        Self {
            providers: RwLock::new(HashMap::new()),
            native_fallbacks: RwLock::new(HashMap::new()),
            startup_loading: Mutex::new(StartupProviderLoading::default()),
            startup_loading_changed: Condvar::new(),
        }
    }

    pub fn begin_startup_provider_loading<I, S>(&self, ids: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut loading = self.startup_loading.lock();
        loading.pending.extend(
            ids.into_iter()
                .map(|id| id.as_ref().trim().to_string())
                .filter(|id| !id.is_empty()),
        );
    }

    pub fn finish_startup_provider_loading(&self) {
        let mut loading = self.startup_loading.lock();
        if loading.pending.is_empty() {
            return;
        }
        loading.pending.clear();
        self.startup_loading_changed.notify_all();
    }

    fn mark_startup_provider_loaded(&self, id: &str) {
        let mut loading = self.startup_loading.lock();
        if loading.pending.remove(id) {
            self.startup_loading_changed.notify_all();
        }
    }

    pub fn wait_for_provider_startup(&self, id: &str) -> ProviderStartupStatus {
        self.wait_for_provider_startup_with_timeout(id, PROVIDER_STARTUP_WAIT)
    }

    pub fn wait_for_provider_startup_with_timeout(
        &self,
        id: &str,
        timeout: Duration,
    ) -> ProviderStartupStatus {
        let deadline = Instant::now() + timeout;
        let mut loading = self.startup_loading.lock();
        while loading.pending.contains(id) {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return ProviderStartupStatus::LoadingTimedOut;
            };
            if self
                .startup_loading_changed
                .wait_for(&mut loading, remaining)
                .timed_out()
            {
                return ProviderStartupStatus::LoadingTimedOut;
            }
        }
        drop(loading);
        if self.with_provider(id, |_| ()).is_some() {
            ProviderStartupStatus::Ready
        } else {
            ProviderStartupStatus::Unknown
        }
    }

    /// The displaced native provider for `id`, if any. Used by JS providers to
    /// delegate capabilities they don't implement (e.g. sqlite-backed chat).
    /// Returns only genuinely native impls — a JS provider must never be handed
    /// back as a fallback, or `JsProvider::native()` could resolve to itself and
    /// recurse forever (#16).
    pub fn native_fallback(&self, id: &str) -> Option<Arc<dyn AgentProvider>> {
        self.native_fallbacks
            .read()
            .get(id)
            .filter(|p| p.is_native())
            .cloned()
    }

    /// Register an agent provider. Overwrites any prior provider with the same
    /// id. The FIRST displaced provider for an id is preserved as a native
    /// fallback (a JS plugin taking authority over a native impl), so the JS
    /// provider can delegate unimplemented capabilities to it. Later
    /// re-registrations (hot-reload) don't disturb the recorded fallback.
    pub fn register_provider(&self, provider: Box<dyn AgentProvider>) {
        let provider: Arc<dyn AgentProvider> = Arc::from(provider);
        let id = provider.id().to_string();
        log::info!("[agent-registry] registered provider: {}", id);
        let mut guard = self.providers.write();
        if let Some(prev) = guard.remove(&id) {
            if prev.is_native() {
                self.native_fallbacks
                    .write()
                    .entry(id.clone())
                    .or_insert(prev);
            }
        }
        guard.insert(id.clone(), provider);
        drop(guard);
        self.mark_startup_provider_loaded(&id);
    }

    /// Register a provider only if `id` is not already present.
    /// Returns `true` if it was added, `false` if a provider with the same
    /// id was already registered (caller's provider is dropped).
    pub fn register_provider_if_absent(&self, provider: Box<dyn AgentProvider>) -> bool {
        let id = provider.id().to_string();
        let mut guard = self.providers.write();
        if guard.contains_key(&id) {
            log::info!(
                "[agent-registry] skipping provider {}: already registered",
                id
            );
            drop(guard);
            self.mark_startup_provider_loaded(&id);
            return false;
        }
        log::info!("[agent-registry] registered provider: {}", id);
        guard.insert(id.clone(), Arc::from(provider));
        drop(guard);
        self.mark_startup_provider_loaded(&id);
        true
    }

    /// Test-only — number of registered providers.
    #[cfg(test)]
    pub fn provider_count(&self) -> usize {
        self.providers.read().len()
    }

    /// Remove a provider by id. Returns true if it existed.
    pub fn unregister_provider(&self, id: &str) -> bool {
        self.providers.write().remove(id).is_some()
    }

    /// Get a provider by ID and execute a closure with it.
    /// Uses a read lock — cheap for concurrent access.
    pub fn with_provider<F, R>(&self, id: &str, f: F) -> Option<R>
    where
        F: FnOnce(&dyn AgentProvider) -> R,
    {
        let guard = self.providers.read();
        guard.get(id).map(|p| f(p.as_ref()))
    }

    /// Clone the `Arc` for a provider so it can be moved into a background task
    /// (e.g. live model discovery on the blocking pool) without holding the
    /// registry lock for the task's duration.
    pub fn provider_arc(&self, id: &str) -> Option<Arc<dyn AgentProvider>> {
        self.providers.read().get(id).cloned()
    }

    /// Clone every registered provider's `Arc`. Used by startup model-catalog
    /// warming to fan out one discovery task per provider.
    pub fn all_provider_arcs(&self) -> Vec<Arc<dyn AgentProvider>> {
        self.providers.read().values().cloned().collect()
    }

    /// Get a provider by ID (e.g. "claude").
    ///
    /// Returns a reference valid for the lifetime of the read lock guard.
    /// For short-lived access patterns, prefer `with_provider()` which
    /// manages the lock scope automatically.
    pub fn get<'a>(
        &'a self,
        id: &str,
    ) -> Option<impl std::ops::Deref<Target = dyn AgentProvider> + 'a> {
        let guard = self.providers.read();
        // must return something that holds the guard alive. Use a mapped guard via parking_lot's RwLockReadGuard::try_map.
        parking_lot::RwLockReadGuard::try_map(guard, |map| map.get(id).map(|b| b.as_ref())).ok()
    }

    /// Detect agent provider from a tab/tab title.
    /// Returns the provider id if found.
    pub fn detect_from_title(&self, title: &str) -> Option<String> {
        let guard = self.providers.read();
        guard
            .values()
            .find(|p| p.detect_from_title(title))
            .map(|p| p.id().to_string())
    }

    /// Returns true when any registered provider's TUI is visible in the
    /// given screen text. Complements the universal glyph check in
    /// `screen_state::screen_has_agent_tui` — each provider owns its own
    /// text fingerprints via `AgentProvider::screen_has_tui`.
    pub fn any_provider_screen_has_tui(&self, text: &str) -> bool {
        let guard = self.providers.read();
        guard.values().any(|p| p.screen_has_tui(text))
    }

    /// Detect agent provider from PTY output.
    /// Returns the provider id if found.
    pub fn detect_from_output(&self, text: &str) -> Option<String> {
        let guard = self.providers.read();
        guard
            .values()
            .find(|p| p.detect_from_output(text))
            .map(|p| p.id().to_string())
    }

    /// Look up the provider id whose `binary_name()` matches the given token.
    /// Used by the PTY input parser to map typed shell commands → agent_type.
    pub fn provider_for_binary(&self, binary: &str) -> Option<String> {
        let guard = self.providers.read();
        guard
            .values()
            .find(|p| p.binary_name() == binary)
            .map(|p| p.id().to_string())
    }

    pub fn has_event_markers(&self, agent_type: &str, chunk: &str) -> bool {
        let guard = self.providers.read();
        guard
            .get(agent_type)
            .map_or(false, |p| p.has_event_markers(chunk))
    }

    pub fn detect_events(
        &self,
        agent_type: &str,
        text: &str,
    ) -> Vec<crate::agent::detector::AgentEvent> {
        let guard = self.providers.read();
        guard
            .get(agent_type)
            .map_or_else(Vec::new, |p| p.detect_events(text))
    }

    pub fn status_bar_rows(&self, agent_type: &str) -> u16 {
        let guard = self.providers.read();
        guard.get(agent_type).map_or(0, |p| p.status_bar_rows())
    }

    pub fn parse_prompt(
        &self,
        agent_type: &str,
        text: &str,
    ) -> Option<crate::multiplexer::types::PendingPrompt> {
        let guard = self.providers.read();
        guard.get(agent_type).and_then(|p| p.parse_prompt(text))
    }

    pub fn is_output_noise(&self, agent_type: &str, text: &str) -> bool {
        let guard = self.providers.read();
        guard
            .get(agent_type)
            .map_or(false, |p| p.is_output_noise(text))
    }

    pub fn build_prompt_response(
        &self,
        agent_type: &str,
        option_index: usize,
        screen: &str,
    ) -> Option<Vec<Vec<u8>>> {
        let guard = self.providers.read();
        guard
            .get(agent_type)
            .map(|p| p.build_prompt_response(option_index, screen))
    }

    /// Check if any providers are registered.
    pub fn is_empty(&self) -> bool {
        self.providers.read().is_empty()
    }

    /// List all registered provider ids.
    pub fn provider_ids(&self) -> Vec<String> {
        self.providers.read().keys().cloned().collect()
    }

    /// Snapshot every registered provider as a `ProviderDescriptor` —
    /// the wire format the frontend reads via `dispatch::Command::ListAgentProviders`.
    ///
    /// Iterates the existing provider map; no separate descriptor cache.
    /// Each `provider.describe()` is documented as pure/cheap, so the
    /// read lock is held only for the iteration and dropped before any
    /// caller serializes the result.
    pub fn list_descriptors(&self) -> Vec<crate::agent::provider::ProviderDescriptor> {
        let guard = self.providers.read();
        let out: Vec<_> = guard.values().map(|p| p.describe()).collect();
        drop(guard);
        out
    }

    /// Locate a session by id across all registered providers. Returns the
    /// provider-id-matching `SessionMetadata` (file_path, cwd, etc.), or
    /// None if nothing claims it. Used by "resume a past session" flows so
    /// callers can open the agent in the RIGHT cwd instead of guessing.
    pub fn find_session(&self, session_id: &str) -> Option<SessionMetadata> {
        let guard = self.providers.read();
        for provider in guard.values() {
            for meta in provider.enumerate_sessions() {
                if meta.session_id == session_id {
                    return Some(meta);
                }
            }
        }
        None
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::agent_claude::ClaudeProvider;

    fn registry_with_claude() -> AgentRegistry {
        let reg = AgentRegistry::new();
        reg.register_provider(Box::new(ClaudeProvider::new()));
        reg
    }

    /// Minimal provider used to exercise the native-fallback bookkeeping.
    /// `native` toggles `is_native()` so we can model both a real native impl
    /// and a JS drop-in without spinning a VM.
    struct FallbackStub {
        id: &'static str,
        native: bool,
    }

    impl crate::agent::provider::AgentProvider for FallbackStub {
        fn id(&self) -> &str {
            self.id
        }
        fn display_name(&self) -> &str {
            self.id
        }
        fn build_launch_command(&self, _: &crate::agent::provider::AgentLaunchParams) -> String {
            String::new()
        }
        fn readiness_strategy(&self) -> crate::agent::provider::ReadinessStrategy {
            crate::agent::provider::ReadinessStrategy::FixedDelay {
                duration: std::time::Duration::from_secs(0),
            }
        }
        fn task_delivery(&self) -> crate::agent::provider::TaskDelivery {
            crate::agent::provider::TaskDelivery::PtyInput
        }
        fn detect_from_title(&self, _: &str) -> bool {
            false
        }
        fn detect_from_output(&self, _: &str) -> bool {
            false
        }
        fn session_file_path(&self, _: &str, _: &str) -> Option<std::path::PathBuf> {
            None
        }
        fn detect_session_id(
            &self,
            _: &str,
            _: &[String],
            _: Option<std::time::Duration>,
        ) -> Option<String> {
            None
        }
        fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
            None
        }
        fn available_models(&self) -> Vec<crate::agent::provider::ModelInfo> {
            crate::plugin::model_catalog::resolve_models(self.id)
        }
        fn is_native(&self) -> bool {
            self.native
        }
    }

    /// #16 regression: a JS-only provider (no native sibling) re-registered on
    /// hot-reload must NOT be captured as its own native fallback. Otherwise
    /// `JsProvider::native()` resolves to itself and capability delegation
    /// recurses until the stack overflows.
    #[test]
    fn js_only_provider_never_becomes_native_fallback() {
        let reg = AgentRegistry::new();
        // (hot-reload seeding tick) — the second displaces the first.
        reg.register_provider(Box::new(FallbackStub {
            id: "drop-in",
            native: false,
        }));
        reg.register_provider(Box::new(FallbackStub {
            id: "drop-in",
            native: false,
        }));
        assert!(
            reg.native_fallback("drop-in").is_none(),
            "a JS drop-in displaced by re-registration must never be a fallback"
        );
    }

    /// A JS provider displacing a genuine native impl still records the native
    /// one as the fallback (the legitimate delegation case), and re-registration
    /// keeps it native.
    #[test]
    fn native_impl_is_preserved_as_fallback_across_reregistration() {
        let reg = AgentRegistry::new();
        reg.register_provider(Box::new(FallbackStub {
            id: "dual",
            native: true,
        }));
        reg.register_provider(Box::new(FallbackStub {
            id: "dual",
            native: false,
        }));
        let fb = reg
            .native_fallback("dual")
            .expect("native fallback recorded");
        assert!(fb.is_native(), "fallback must be the native impl");
        // Hot-reload re-registers the JS provider again — fallback stays native.
        reg.register_provider(Box::new(FallbackStub {
            id: "dual",
            native: false,
        }));
        assert!(reg.native_fallback("dual").unwrap().is_native());
    }

    #[test]
    fn test_empty_registry() {
        let reg = AgentRegistry::new();
        assert!(reg.is_empty());
        assert!(reg.detect_from_title("Claude Code session").is_none());
    }

    #[test]
    fn test_register_and_get() {
        let reg = registry_with_claude();
        assert!(!reg.is_empty());
        assert!(reg
            .with_provider("claude", |p| p.id().to_string())
            .is_some());
    }

    #[test]
    fn test_unregister() {
        let reg = registry_with_claude();
        assert!(reg.unregister_provider("claude"));
        assert!(reg.is_empty());
        assert!(!reg.unregister_provider("claude"));
    }

    #[test]
    fn test_registry_get_unknown() {
        let reg = AgentRegistry::new();
        assert!(reg.with_provider("nonexistent", |_| ()).is_none());
    }

    #[test]
    fn startup_wait_unblocks_when_provider_registers() {
        let reg = Arc::new(AgentRegistry::new());
        reg.begin_startup_provider_loading(["slow"]);

        let worker_reg = reg.clone();
        let worker = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(25));
            worker_reg.register_provider(Box::new(FallbackStub {
                id: "slow",
                native: false,
            }));
        });

        assert_eq!(
            reg.wait_for_provider_startup_with_timeout("slow", Duration::from_secs(1)),
            ProviderStartupStatus::Ready
        );
        worker
            .join()
            .expect("provider registration thread panicked");
    }

    #[test]
    fn startup_wait_reports_loading_before_unknown() {
        let reg = AgentRegistry::new();
        reg.begin_startup_provider_loading(["slow"]);

        assert_eq!(
            reg.wait_for_provider_startup_with_timeout("slow", Duration::from_millis(1)),
            ProviderStartupStatus::LoadingTimedOut
        );
        reg.finish_startup_provider_loading();
        assert_eq!(
            reg.wait_for_provider_startup_with_timeout("slow", Duration::from_millis(1)),
            ProviderStartupStatus::Unknown
        );
    }

    #[test]
    fn register_if_absent_clears_startup_loading_for_pinned_native() {
        let reg = AgentRegistry::new();
        reg.register_provider(Box::new(FallbackStub {
            id: "pinned",
            native: true,
        }));
        reg.begin_startup_provider_loading(["pinned"]);

        assert!(!reg.register_provider_if_absent(Box::new(FallbackStub {
            id: "pinned",
            native: false,
        })));
        assert_eq!(
            reg.wait_for_provider_startup_with_timeout("pinned", Duration::from_millis(1)),
            ProviderStartupStatus::Ready
        );
    }

    #[test]
    fn test_detect_from_title() {
        let reg = registry_with_claude();
        let id = reg.detect_from_title("Claude Code session");
        assert_eq!(id.as_deref(), Some("claude"));
        assert!(reg.detect_from_title("bash shell").is_none());
    }

    #[test]
    fn test_detect_from_output() {
        let reg = registry_with_claude();
        let id = reg.detect_from_output("Do you want to proceed?");
        assert_eq!(id.as_deref(), Some("claude"));
        assert!(reg.detect_from_output("normal output").is_none());
    }

    #[test]
    fn test_provider_ids() {
        let reg = registry_with_claude();
        assert_eq!(reg.provider_ids(), vec!["claude".to_string()]);
    }

    #[test]
    fn list_descriptors_returns_one_per_provider() {
        let reg = registry_with_claude();
        let descs = reg.list_descriptors();
        assert_eq!(descs.len(), 1);
        assert_eq!(descs[0].id, "claude");
        assert_eq!(descs[0].display_name, "Claude Code");
        assert!(!descs[0].available_models.is_empty());
        assert!(!descs[0].settings_schema.is_empty());
    }

    #[test]
    fn listed_models_share_the_live_cache_registry() {
        let id = "test-listed-models-share-live-cache";
        crate::plugin::model_catalog::register(
            id,
            crate::plugin::model_catalog::RegisteredDefaults {
                models: vec![crate::agent::provider::ModelInfo {
                    id: "phantom-model".into(),
                    display_name: "Phantom Model".into(),
                    description: None,
                    available: None,
                }],
                default: Some("phantom-model".into()),
            },
        );
        crate::agent::model_catalog::put_live(
            id,
            vec![crate::agent::provider::ModelInfo {
                id: "cache-model".into(),
                display_name: "Cache Model".into(),
                description: None,
                available: None,
            }],
            Some("cache-model".into()),
        );

        let reg = AgentRegistry::new();
        reg.register_provider(Box::new(FallbackStub { id, native: false }));
        let cached: Vec<_> = crate::agent::model_catalog::resolve_models(id)
            .into_iter()
            .map(|model| model.id)
            .collect();
        let listed: Vec<_> = reg
            .list_descriptors()
            .remove(0)
            .available_models
            .into_iter()
            .map(|model| model.id)
            .collect();

        let _ = std::fs::remove_file(
            crate::agent::model_catalog::models_dir().join(format!("{id}.cache.json")),
        );
        assert_eq!(listed, cached, "agent models must list the cached snapshot");
    }

    #[test]
    fn list_descriptors_with_two_providers() {
        let reg = AgentRegistry::new();
        reg.register_provider(Box::new(ClaudeProvider::new()));
        reg.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
            "codex",
        )));
        let descs = reg.list_descriptors();
        assert_eq!(descs.len(), 2);
        let ids: std::collections::HashSet<&str> = descs.iter().map(|d| d.id.as_str()).collect();
        assert!(ids.contains("claude"));
        assert!(ids.contains("codex"));
    }

    #[test]
    fn restore_dispatch_builds_resume_for_every_provider() {
        let reg = AgentRegistry::new();
        reg.register_provider(Box::new(ClaudeProvider::new()));
        reg.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
            "codex",
        )));
        reg.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
            "opencode",
        )));
        for (id, sid) in [
            ("claude", "f00fc0dd-be1c-4d43-bb1d-212c923b75c1"),
            ("codex", "019e15cd-1527-7e21-9477-2631a52e499a"),
            ("opencode", "ses_1ea31e6d2ffe8WrqZl6g4mc6l2"),
        ] {
            let cmd = reg
                .with_provider(id, |p| p.build_resume_command(sid, true))
                .flatten()
                .unwrap_or_else(|| panic!("provider {} returned no resume command", id));
            assert!(
                cmd.contains(sid),
                "provider {} resume command missing sid: {}",
                id,
                cmd
            );
        }
    }

    #[test]
    fn restore_dispatch_falls_back_to_fresh_launch_when_no_resume() {
        use crate::agent::provider::AgentLaunchParams;
        let reg = AgentRegistry::new();
        reg.register_provider(Box::new(ClaudeProvider::new()));
        reg.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
            "codex",
        )));
        reg.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
            "opencode",
        )));
        for id in ["claude", "codex", "opencode"] {
            let cmd = reg.with_provider(id, |p| {
                p.build_launch_command(&AgentLaunchParams {
                    skip_permissions: true,
                    ..Default::default()
                })
            });
            let cmd = cmd.unwrap_or_else(|| panic!("provider {} missing", id));
            assert!(
                !cmd.is_empty(),
                "provider {} returned empty launch command",
                id
            );
        }
    }

    #[test]
    fn list_descriptors_serializes_to_json_array() {
        let reg = registry_with_claude();
        let descs = reg.list_descriptors();
        let json = serde_json::to_value(&descs).expect("serialize");
        assert!(json.is_array());
        assert_eq!(json.as_array().unwrap().len(), 1);
    }
}
