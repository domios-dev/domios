use std::collections::HashMap;

use super::claude::ClaudeProvider;
use super::provider::AgentProvider;

/// Registry of available agent providers.
pub struct AgentRegistry {
    providers: HashMap<String, Box<dyn AgentProvider>>,
}

impl AgentRegistry {
    /// Create a new registry with all built-in providers.
    pub fn new() -> Self {
        let mut providers: HashMap<String, Box<dyn AgentProvider>> = HashMap::new();
        let claude = ClaudeProvider::new();
        providers.insert(claude.id().to_string(), Box::new(claude));
        Self { providers }
    }

    /// Get a provider by ID (e.g. "claude").
    pub fn get(&self, id: &str) -> Option<&dyn AgentProvider> {
        self.providers.get(id).map(|b| b.as_ref())
    }

    /// Detect agent provider from a pane/tab title.
    pub fn detect_from_title(&self, title: &str) -> Option<&dyn AgentProvider> {
        self.providers
            .values()
            .find(|p| p.detect_from_title(title))
            .map(|b| b.as_ref())
    }

    /// Detect agent provider from PTY output.
    pub fn detect_from_output(&self, text: &str) -> Option<&dyn AgentProvider> {
        self.providers
            .values()
            .find(|p| p.detect_from_output(text))
            .map(|b| b.as_ref())
    }

    /// Quick check: does this chunk contain markers worth full detection?
    pub fn has_event_markers(&self, agent_type: &str, chunk: &str) -> bool {
        if let Some(provider) = self.get(agent_type) {
            provider.has_event_markers(chunk)
        } else {
            false
        }
    }

    /// Detect events using the appropriate provider for a given agent type.
    pub fn detect_events(&self, agent_type: &str, text: &str) -> Vec<crate::agent::detector::AgentEvent> {
        if let Some(provider) = self.get(agent_type) {
            provider.detect_events(text)
        } else {
            Vec::new()
        }
    }

    /// Parse a permission prompt using the appropriate provider.
    pub fn parse_prompt(&self, agent_type: &str, text: &str) -> Option<(String, Vec<(String, String)>)> {
        if let Some(provider) = self.get(agent_type) {
            provider.parse_prompt(text)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_has_claude() {
        let reg = AgentRegistry::new();
        let p = reg.get("claude");
        assert!(p.is_some());
        assert_eq!(p.unwrap().id(), "claude");
    }

    #[test]
    fn test_registry_get_unknown() {
        let reg = AgentRegistry::new();
        assert!(reg.get("nonexistent").is_none());
    }

    #[test]
    fn test_detect_from_title() {
        let reg = AgentRegistry::new();
        let p = reg.detect_from_title("Claude Code session");
        assert!(p.is_some());
        assert_eq!(p.unwrap().id(), "claude");

        assert!(reg.detect_from_title("bash shell").is_none());
    }

    #[test]
    fn test_detect_from_output() {
        let reg = AgentRegistry::new();
        let p = reg.detect_from_output("Do you want to proceed?");
        assert!(p.is_some());
        assert_eq!(p.unwrap().id(), "claude");

        assert!(reg.detect_from_output("normal output").is_none());
    }
}
