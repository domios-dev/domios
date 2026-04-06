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
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::claude::ClaudeProvider;

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
}
