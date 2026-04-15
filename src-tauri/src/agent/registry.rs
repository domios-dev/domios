use std::collections::HashMap;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use super::provider::{AgentProvider, SessionMetadata};

/// One matching snippet from a session message, with enough surrounding
/// context for a human to decide whether to resume the session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnippet {
    pub role: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<i64>,
}

/// One session search hit: metadata + the top-N snippets that contained
/// the query terms. Provider-agnostic — `meta.provider` tags the origin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSearchHit {
    pub meta: SessionMetadata,
    pub snippets: Vec<SessionSnippet>,
    pub match_count: usize,
}

/// Filters/limits for a cross-provider session search.
#[derive(Debug, Clone, Default)]
pub struct SessionSearchOpts {
    /// Each term must appear (case-insensitive) in at least one message of
    /// a session for it to match. `terms` is typically the whitespace split
    /// of a user query.
    pub terms: Vec<String>,
    /// Restrict to a specific provider id (e.g. "claude"). `None` = all.
    pub provider: Option<String>,
    /// Restrict to sessions whose recorded `project_path` equals this value.
    pub project_path: Option<String>,
    /// Max number of hits returned, ordered by most-recently-modified first.
    pub limit: usize,
    /// Max snippets returned per session.
    pub max_snippets: usize,
}

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

    /// Cross-provider session search. Iterates every registered provider,
    /// calls `enumerate_sessions()` + `parse_session_messages()`, and returns
    /// merged hits sorted by most-recently-modified. Providers that haven't
    /// implemented the trait methods silently contribute nothing.
    ///
    /// Kept on-demand (no persistent index) for v1 — JSONLs are small and
    /// session counts are typically <1000. Upgrade to FTS5 when perf bites.
    pub fn search_sessions(&self, opts: &SessionSearchOpts) -> Vec<SessionSearchHit> {
        if opts.terms.is_empty() {
            return Vec::new();
        }
        let lowered: Vec<String> = opts.terms.iter().map(|t| t.to_lowercase()).collect();
        let limit = if opts.limit == 0 { 10 } else { opts.limit };
        let max_snippets = if opts.max_snippets == 0 { 3 } else { opts.max_snippets };

        let guard = self.providers.read();
        let mut hits: Vec<SessionSearchHit> = Vec::new();
        for provider in guard.values() {
            if let Some(ref pid) = opts.provider {
                if pid != provider.id() {
                    continue;
                }
            }
            for meta in provider.enumerate_sessions() {
                if let Some(ref proj) = opts.project_path {
                    if meta.project_path.as_deref() != Some(proj.as_str()) {
                        continue;
                    }
                }
                let messages = provider.parse_session_messages(std::path::Path::new(&meta.file_path));
                let mut matched_msgs = 0usize;
                let mut snippets: Vec<SessionSnippet> = Vec::new();
                for msg in &messages {
                    if msg.is_meta {
                        continue;
                    }
                    let text_lc = msg.text.to_lowercase();
                    if !lowered.iter().all(|t| text_lc.contains(t)) {
                        continue;
                    }
                    matched_msgs += 1;
                    if snippets.len() < max_snippets {
                        let pos = lowered
                            .iter()
                            .filter_map(|t| text_lc.find(t))
                            .min()
                            .unwrap_or(0);
                        let start = msg.text.floor_char_boundary(pos.saturating_sub(80));
                        let end_raw = (pos + 120).min(msg.text.len());
                        let end = msg.text.floor_char_boundary(end_raw);
                        let mut snippet = msg.text[start..end].to_string();
                        if start > 0 {
                            snippet.insert_str(0, "…");
                        }
                        if end < msg.text.len() {
                            snippet.push('…');
                        }
                        snippets.push(SessionSnippet {
                            role: msg.role.clone(),
                            text: snippet,
                            ts: msg.ts,
                        });
                    }
                }
                if !snippets.is_empty() {
                    hits.push(SessionSearchHit { meta, snippets, match_count: matched_msgs });
                }
            }
        }
        drop(guard);

        hits.sort_by(|a, b| {
            b.meta
                .modified_ts
                .unwrap_or(0)
                .cmp(&a.meta.modified_ts.unwrap_or(0))
        });
        hits.truncate(limit);
        hits
    }
}

/// Crate-local shim: `str::floor_char_boundary` is still unstable on stable
/// Rust, so provide a minimal equivalent. Returns the largest char boundary
/// index <= `index`. Needed to slice UTF-8 safely inside a snippet window.
trait FloorCharBoundary {
    fn floor_char_boundary(&self, index: usize) -> usize;
}
impl FloorCharBoundary for str {
    fn floor_char_boundary(&self, index: usize) -> usize {
        let mut i = index.min(self.len());
        while i > 0 && !self.is_char_boundary(i) {
            i -= 1;
        }
        i
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
