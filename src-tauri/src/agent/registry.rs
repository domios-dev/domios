use std::collections::HashMap;

use parking_lot::RwLock;

use super::provider::AgentProvider;

/// Registry of available agent providers.
///
/// Uses interior mutability (RwLock) so providers can be registered
/// dynamically by plugins while the registry is shared via `Arc<AgentRegistry>`.
pub struct AgentRegistry {
    providers: RwLock<HashMap<String, Box<dyn AgentProvider>>>,
}

impl AgentRegistry {
    /// Create an empty registry. Providers are added via `register_provider()`.
    pub fn new() -> Self {
        Self {
            providers: RwLock::new(HashMap::new()),
        }
    }

    /// Register an agent provider. Overwrites any prior provider with the same id.
    pub fn register_provider(&self, provider: Box<dyn AgentProvider>) {
        let id = provider.id().to_string();
        log::info!("[agent-registry] registered provider: {}", id);
        self.providers.write().insert(id, provider);
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

    /// Get a provider by ID (e.g. "claude").
    ///
    /// Returns a reference valid for the lifetime of the read lock guard.
    /// For short-lived access patterns, prefer `with_provider()` which
    /// manages the lock scope automatically.
    pub fn get<'a>(&'a self, id: &str) -> Option<impl std::ops::Deref<Target = dyn AgentProvider> + 'a> {
        let guard = self.providers.read();
        // We need to return something that holds the guard alive.
        // Use a mapped guard via parking_lot's RwLockReadGuard::try_map.
        parking_lot::RwLockReadGuard::try_map(guard, |map| {
            map.get(id).map(|b| b.as_ref())
        })
        .ok()
    }

    /// Detect agent provider from a pane/tab title.
    /// Returns the provider id if found.
    pub fn detect_from_title(&self, title: &str) -> Option<String> {
        let guard = self.providers.read();
        guard
            .values()
            .find(|p| p.detect_from_title(title))
            .map(|p| p.id().to_string())
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

    pub fn has_event_markers(&self, agent_type: &str, chunk: &str) -> bool {
        let guard = self.providers.read();
        guard.get(agent_type).map_or(false, |p| p.has_event_markers(chunk))
    }

    pub fn detect_events(&self, agent_type: &str, text: &str) -> Vec<crate::agent::detector::AgentEvent> {
        let guard = self.providers.read();
        guard.get(agent_type).map_or_else(Vec::new, |p| p.detect_events(text))
    }

    pub fn status_bar_rows(&self, agent_type: &str) -> u16 {
        let guard = self.providers.read();
        guard.get(agent_type).map_or(0, |p| p.status_bar_rows())
    }

    pub fn parse_prompt(&self, agent_type: &str, text: &str) -> Option<(String, Vec<(String, String)>)> {
        let guard = self.providers.read();
        guard.get(agent_type).and_then(|p| p.parse_prompt(text))
    }

    pub fn is_output_noise(&self, agent_type: &str, text: &str) -> bool {
        let guard = self.providers.read();
        guard.get(agent_type).map_or(false, |p| p.is_output_noise(text))
    }

    pub fn build_prompt_response(&self, agent_type: &str, option_index: usize) -> Option<Vec<Vec<u8>>> {
        let guard = self.providers.read();
        guard.get(agent_type).map(|p| p.build_prompt_response(option_index))
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
        assert!(reg.with_provider("claude", |p| p.id().to_string()).is_some());
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
    fn list_descriptors_with_two_providers() {
        use crate::plugin::agent_codex::CodexProvider;
        let reg = AgentRegistry::new();
        reg.register_provider(Box::new(ClaudeProvider::new()));
        reg.register_provider(Box::new(CodexProvider::new()));
        let descs = reg.list_descriptors();
        assert_eq!(descs.len(), 2);
        let ids: std::collections::HashSet<&str> =
            descs.iter().map(|d| d.id.as_str()).collect();
        assert!(ids.contains("claude"));
        assert!(ids.contains("codex"));
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
