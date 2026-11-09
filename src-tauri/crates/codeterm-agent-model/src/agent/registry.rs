use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex, RwLock};

use super::classify_input::StatusBarRowsSource;
use super::provider::{AgentProvider, SessionMetadata};

mod timing;
use timing::RegistryTiming;

pub use super::registry_search::{
    SessionMatchMode, SessionSearchError, SessionSearchHit, SessionSearchOpts, SessionSearchResult,
    SessionSearchWarning, SessionSkipReason, SessionSnippet, DEFAULT_SESSION_FILE_BYTE_CAP,
};

pub struct AgentRegistry {
    providers: RwLock<HashMap<String, RegisteredProvider>>,
    /// First provider displaced for an id (the native one a JS plugin took
    /// authority over). Kept so a JS plugin can delegate capability methods it
    /// doesn't implement to the native impl. Never overwritten by later
    /// re-registrations (hot-reload), so the fallback survives plugin reloads.
    native_fallbacks: RwLock<HashMap<String, Arc<dyn AgentProvider>>>,
    startup_loading: Mutex<StartupProviderLoading>,
    startup_loading_changed: Condvar,
}

#[derive(Clone, Copy)]
pub struct ProviderStateCapabilities {
    pub reconciles_status: bool,
    pub structured_prompt_server: bool,
}

impl ProviderStateCapabilities {
    fn capture(provider: &dyn AgentProvider) -> Self {
        Self {
            reconciles_status: provider.supports_agent_state_status_reconciliation(),
            structured_prompt_server: provider.needs_structured_prompt_server(),
        }
    }
}

#[derive(Default)]
pub struct ProviderStateSnapshot(HashMap<String, ProviderStateCapabilities>);

impl ProviderStateSnapshot {
    pub fn reconciles_status(&self, provider: Option<&str>) -> bool {
        provider
            .and_then(|id| self.0.get(id))
            .is_some_and(|state| state.reconciles_status)
    }

    pub fn owns_structured_state(&self, provider: Option<&str>) -> bool {
        provider
            .and_then(|id| self.0.get(id))
            .is_some_and(|state| state.reconciles_status && state.structured_prompt_server)
    }
}

struct RegisteredProvider {
    provider: Arc<dyn AgentProvider>,
    native: bool,
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
        self.native_fallbacks.read().get(id).cloned()
    }

    /// Register an agent provider. Overwrites any prior provider with the same
    /// id. The FIRST displaced provider for an id is preserved as a native
    /// fallback (a JS plugin taking authority over a native impl), so the JS
    /// provider can delegate unimplemented capabilities to it. Later
    /// re-registrations (hot-reload) don't disturb the recorded fallback.
    pub fn register_provider(&self, provider: Box<dyn AgentProvider>) {
        let provider: Arc<dyn AgentProvider> = Arc::from(provider);
        let id = provider.id().to_string();
        let native = provider.is_native();
        log::info!("[agent-registry] registered provider: {}", id);
        let timing = RegistryTiming::new("register", &id);
        let mut guard = self.providers.write();
        let timing = timing.acquired();
        let previous = guard.insert(id.clone(), RegisteredProvider { provider, native });
        if let Some(entry) = previous.as_ref().filter(|entry| entry.native) {
            self.native_fallbacks
                .write()
                .entry(id.clone())
                .or_insert_with(|| Arc::clone(&entry.provider));
        }
        drop(guard);
        drop(previous);
        drop(timing);
        crate::agent::model_catalog::invalidate(&id);
        self.mark_startup_provider_loaded(&id);
    }

    /// Register a provider only if `id` is not already present.
    /// Returns `true` if it was added, `false` if a provider with the same
    /// id was already registered (caller's provider is dropped).
    pub fn register_provider_if_absent(&self, provider: Box<dyn AgentProvider>) -> bool {
        let id = provider.id().to_string();
        let native = provider.is_native();
        let timing = RegistryTiming::new("register_if_absent", &id);
        let mut guard = self.providers.write();
        let timing = timing.acquired();
        if guard.contains_key(&id) {
            log::info!(
                "[agent-registry] skipping provider {}: already registered",
                id
            );
            drop(guard);
            drop(timing);
            self.mark_startup_provider_loaded(&id);
            return false;
        }
        log::info!("[agent-registry] registered provider: {}", id);
        guard.insert(
            id.clone(),
            RegisteredProvider {
                provider: Arc::from(provider),
                native,
            },
        );
        drop(guard);
        drop(timing);
        crate::agent::model_catalog::invalidate(&id);
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
        let timing = RegistryTiming::new("unregister", id);
        let mut guard = self.providers.write();
        let timing = timing.acquired();
        let removed = guard.remove(id);
        drop(guard);
        drop(timing);
        let existed = removed.is_some();
        drop(removed);
        if existed {
            crate::agent::model_catalog::invalidate(id);
        }
        existed
    }

    #[track_caller]
    pub fn with_provider<F, R>(&self, id: &str, f: F) -> Option<R>
    where
        F: FnOnce(&dyn AgentProvider) -> R,
    {
        self.provider_arc(id).map(|provider| f(provider.as_ref()))
    }

    /// Clone the `Arc` for a provider so it can be moved into a background task
    /// (e.g. live model discovery on the blocking pool) without holding the
    /// registry lock for the task's duration.
    pub fn provider_arc(&self, id: &str) -> Option<Arc<dyn AgentProvider>> {
        let timing = RegistryTiming::new("provider_arc", id);
        let guard = self.providers.read();
        let timing = timing.acquired();
        let provider = guard.get(id).map(|entry| Arc::clone(&entry.provider));
        drop(guard);
        drop(timing);
        provider
    }

    /// Clone every registered provider's `Arc`. Used by startup model-catalog
    /// warming to fan out one discovery task per provider.
    pub fn all_provider_arcs(&self) -> Vec<Arc<dyn AgentProvider>> {
        self.providers
            .read()
            .values()
            .map(|entry| Arc::clone(&entry.provider))
            .collect()
    }

    pub fn state_snapshot(&self) -> ProviderStateSnapshot {
        ProviderStateSnapshot(
            self.all_provider_arcs()
                .into_iter()
                .map(|provider| {
                    (
                        provider.id().to_string(),
                        ProviderStateCapabilities::capture(provider.as_ref()),
                    )
                })
                .collect(),
        )
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn AgentProvider>> {
        self.provider_arc(id)
    }

    /// Detect agent provider from a tab/tab title.
    /// Returns the provider id if found.
    pub fn detect_from_title(&self, title: &str) -> Option<String> {
        self.all_provider_arcs()
            .iter()
            .find(|p| p.detect_from_title(title))
            .map(|p| p.id().to_string())
    }

    /// Returns true when any registered provider's TUI is visible in the
    /// given screen text. Complements the universal glyph check in
    /// `screen_state::screen_has_agent_tui` — each provider owns its own
    /// text fingerprints via `AgentProvider::screen_has_tui`.
    pub fn any_provider_screen_has_tui(&self, text: &str) -> bool {
        self.all_provider_arcs()
            .iter()
            .any(|p| p.screen_has_tui(text))
    }

    /// Detect agent provider from PTY output.
    /// Returns the provider id if found.
    pub fn detect_from_output(&self, text: &str) -> Option<String> {
        self.detect_from_output_for(None, text)
    }

    pub fn detect_from_output_for(&self, agent_type: Option<&str>, text: &str) -> Option<String> {
        let providers = self.all_provider_arcs();
        if let Some(provider) =
            agent_type.and_then(|id| providers.iter().find(|provider| provider.id() == id))
        {
            if provider.detect_from_output(text) {
                return Some(provider.id().to_string());
            }
        }
        providers
            .iter()
            .filter(|p| Some(p.id()) != agent_type)
            .find(|p| p.detect_from_output(text))
            .map(|p| p.id().to_string())
    }

    /// Look up the provider id whose `binary_name()` matches the given token.
    /// Used by the PTY input parser to map typed shell commands → agent_type.
    pub fn provider_for_binary(&self, binary: &str) -> Option<String> {
        self.all_provider_arcs()
            .iter()
            .find(|p| p.binary_name() == binary)
            .map(|p| p.id().to_string())
    }

    pub fn has_event_markers(&self, agent_type: &str, chunk: &str) -> bool {
        self.provider_arc(agent_type)
            .map_or(false, |p| p.has_event_markers(chunk))
    }

    pub fn detect_events(
        &self,
        agent_type: &str,
        text: &str,
    ) -> Vec<crate::agent::detector::AgentEvent> {
        self.provider_arc(agent_type)
            .map_or_else(Vec::new, |p| p.detect_events(text))
    }

    /// `None` when the id resolves to no provider. Never a zero standing in for
    /// "unknown" — a caller that needs the band must handle the refusal.
    pub fn try_status_bar_rows(&self, agent_type: &str) -> Option<u16> {
        self.provider_arc(agent_type).map(|p| p.status_bar_rows())
    }

    pub fn parse_prompt(
        &self,
        agent_type: &str,
        text: &str,
    ) -> Option<crate::multiplexer::types::PendingPrompt> {
        self.provider_arc(agent_type).and_then(|provider| {
            super::interactive_prompt::parse_live_prompt(provider.as_ref(), text)
                .map(|(prompt, _)| prompt)
        })
    }

    pub fn parse_interactive_prompt(
        &self,
        agent_type: &str,
        text: &str,
    ) -> Option<(
        crate::multiplexer::types::PendingPrompt,
        codeterm_types::pty::PromptKind,
    )> {
        let provider = self.provider_arc(agent_type)?;
        super::interactive_prompt::parse_live_prompt(provider.as_ref(), text)
    }

    pub fn is_output_noise(&self, agent_type: &str, text: &str) -> bool {
        self.provider_arc(agent_type)
            .map_or(false, |p| p.is_output_noise(text))
    }

    pub fn build_prompt_response(
        &self,
        agent_type: &str,
        option_index: usize,
        screen: &str,
    ) -> Option<Vec<Vec<u8>>> {
        self.provider_arc(agent_type)
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

    /// Snapshot every registered provider as a `ProviderDescriptor` — the wire
    /// format the frontend reads via `dispatch::Command::ListAgentProviders`.
    ///
    /// Registration says a provider exists; only the launch-binary probe says
    /// it can run here, so enumeration observes it outside the registry lock.
    pub fn list_descriptors(&self) -> Vec<crate::agent::provider::ProviderDescriptor> {
        let providers = self.all_provider_arcs();
        let described: Vec<_> = providers
            .iter()
            .map(|p| {
                let descriptor = p.describe();
                let launch_binary = descriptor
                    .agent_runtime
                    .as_ref()
                    .map(|runtime| runtime.command.binary.clone())
                    .unwrap_or_else(|| p.binary_name().to_string());
                (descriptor, launch_binary)
            })
            .collect();
        described
            .into_iter()
            .map(|(mut descriptor, launch_binary)| {
                descriptor.availability =
                    crate::agent::provider::observe_launch_binary(Some(&launch_binary))
                        .availability;
                descriptor
            })
            .collect()
    }

    /// Resolve an id to the provider record owning exactly it, from each provider's
    /// own per-session record; never enumerates history or holds the lock across I/O.
    pub fn find_session(&self, session_id: &str) -> Option<SessionMetadata> {
        self.all_provider_arcs().into_iter().find_map(|provider| {
            provider
                .find_session(session_id)
                .filter(|meta| meta.session_id == session_id)
        })
    }
}

#[cfg(test)]
mod concurrency_tests;
#[cfg(test)]
mod find_session_tests;

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;

impl StatusBarRowsSource for AgentRegistry {
    fn status_bar_rows_for(&self, agent_type: &str) -> Option<u16> {
        self.try_status_bar_rows(agent_type)
    }
}
