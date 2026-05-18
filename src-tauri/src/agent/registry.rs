use std::collections::HashMap;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use super::provider::{AgentProvider, SessionMetadata};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnippet {
    pub role: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSearchHit {
    pub meta: SessionMetadata,
    pub snippets: Vec<SessionSnippet>,
    pub match_count: usize,
}

#[derive(Debug, Clone, Default)]
pub struct SessionSearchOpts {
    pub terms: Vec<String>,
    pub provider: Option<String>,
    pub project_path: Option<String>,
    pub limit: usize,
    pub max_snippets: usize,
}

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

    /// Register a provider only if `id` is not already present.
    /// Returns `true` if it was added, `false` if a provider with the same
    /// id was already registered (caller's provider is dropped).
    pub fn register_provider_if_absent(&self, provider: Box<dyn AgentProvider>) -> bool {
        let id = provider.id().to_string();
        let mut guard = self.providers.write();
        if guard.contains_key(&id) {
            log::info!("[agent-registry] skipping provider {}: already registered", id);
            return false;
        }
        log::info!("[agent-registry] registered provider: {}", id);
        guard.insert(id, provider);
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
        // Use ASCII-lowercase so the lowered string stays byte-for-byte aligned
        // with the original. `str::to_lowercase` can change byte AND char count
        // (`ß → ss`, `İ → i̇`), which would misalign snippet windows when we
        // translate a lowercased-string position back into the original.
        let lowered_terms: Vec<String> =
            opts.terms.iter().map(|t| t.to_ascii_lowercase()).collect();
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
                    let text_lc = msg.text.to_ascii_lowercase();
                    if !lowered_terms.iter().all(|t| text_lc.contains(t)) {
                        continue;
                    }
                    matched_msgs += 1;
                    if snippets.len() < max_snippets {
                        // Positions in `text_lc` map 1:1 to bytes in `msg.text`
                        // thanks to ascii-lowercasing.
                        let pos = lowered_terms
                            .iter()
                            .filter_map(|t| text_lc.find(t))
                            .min()
                            .unwrap_or(0);
                        let start = floor_char_boundary(&msg.text, pos.saturating_sub(80));
                        let end = floor_char_boundary(&msg.text, (pos + 120).min(msg.text.len()));
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

/// Largest char boundary index ≤ `index`. Shim for the still-unstable
/// `str::floor_char_boundary`; needed to slice UTF-8 safely inside the
/// snippet window in `search_sessions`.
fn floor_char_boundary(s: &str, index: usize) -> usize {
    let mut i = index.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
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
    fn restore_dispatch_builds_resume_for_every_provider() {
        use crate::plugin::agent_codex::CodexProvider;
        use crate::plugin::agent_opencode::OpenCodeProvider;
        let reg = AgentRegistry::new();
        reg.register_provider(Box::new(ClaudeProvider::new()));
        reg.register_provider(Box::new(CodexProvider::new()));
        reg.register_provider(Box::new(OpenCodeProvider::new()));
        for (id, sid) in [
            ("claude", "f00fc0dd-be1c-4d43-bb1d-212c923b75c1"),
            ("codex", "019e15cd-1527-7e21-9477-2631a52e499a"),
            ("opencode", "ses_1ea31e6d2ffe8WrqZl6g4mc6l2"),
        ] {
            let cmd = reg.with_provider(id, |p| p.build_resume_command(sid, true))
                .flatten()
                .unwrap_or_else(|| panic!("provider {} returned no resume command", id));
            assert!(cmd.contains(sid), "provider {} resume command missing sid: {}", id, cmd);
        }
    }

    #[test]
    fn restore_dispatch_falls_back_to_fresh_launch_when_no_resume() {
        use crate::agent::provider::AgentLaunchParams;
        use crate::plugin::agent_codex::CodexProvider;
        use crate::plugin::agent_opencode::OpenCodeProvider;
        let reg = AgentRegistry::new();
        reg.register_provider(Box::new(ClaudeProvider::new()));
        reg.register_provider(Box::new(CodexProvider::new()));
        reg.register_provider(Box::new(OpenCodeProvider::new()));
        for id in ["claude", "codex", "opencode"] {
            let cmd = reg.with_provider(id, |p| p.build_launch_command(&AgentLaunchParams {
                skip_permissions: true,
                ..Default::default()
            }));
            let cmd = cmd.unwrap_or_else(|| panic!("provider {} missing", id));
            assert!(!cmd.is_empty(), "provider {} returned empty launch command", id);
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

    // UTF-8 safety: snippet windowing must never slice mid-codepoint.
    #[test]
    fn floor_char_boundary_handles_utf8() {
        let s = "Привет мир";
        assert_eq!(super::floor_char_boundary(s, 0), 0);
        assert_eq!(super::floor_char_boundary(s, s.len()), s.len());
        for i in 1..s.len() {
            let b = super::floor_char_boundary(s, i);
            assert!(s.is_char_boundary(b));
            assert!(b <= i);
        }
    }

    struct StubSearchProvider {
        sessions: Vec<SessionMetadata>,
        messages: std::collections::HashMap<String, Vec<crate::agent::provider::SessionMessage>>,
    }

    impl crate::agent::provider::AgentProvider for StubSearchProvider {
        fn id(&self) -> &str { "stub" }
        fn display_name(&self) -> &str { "Stub" }
        fn build_launch_command(&self, _: &crate::agent::provider::AgentLaunchParams) -> String { String::new() }
        fn readiness_strategy(&self) -> crate::agent::provider::ReadinessStrategy {
            crate::agent::provider::ReadinessStrategy::FixedDelay { duration: std::time::Duration::from_secs(0) }
        }
        fn task_delivery(&self) -> crate::agent::provider::TaskDelivery {
            crate::agent::provider::TaskDelivery::PtyInput
        }
        fn detect_from_title(&self, _: &str) -> bool { false }
        fn detect_from_output(&self, _: &str) -> bool { false }
        fn session_file_path(&self, _: &str, _: &str) -> Option<std::path::PathBuf> { None }
        fn detect_session_id(&self, _: &str, _: &[String], _: Option<std::time::Duration>) -> Option<String> { None }
        fn build_resume_command(&self, _: &str, _: bool) -> Option<String> { None }
        fn enumerate_sessions(&self) -> Vec<SessionMetadata> { self.sessions.clone() }
        fn parse_session_messages(&self, path: &std::path::Path) -> Vec<crate::agent::provider::SessionMessage> {
            self.messages.get(path.to_str().unwrap_or("")).cloned().unwrap_or_default()
        }
    }

    fn stub_registry(sessions: Vec<SessionMetadata>, msgs: Vec<(&str, Vec<(&str, &str)>)>) -> AgentRegistry {
        let mut messages = std::collections::HashMap::new();
        for (file, list) in msgs {
            messages.insert(
                file.to_string(),
                list.into_iter().map(|(role, text)| crate::agent::provider::SessionMessage {
                    role: role.to_string(),
                    text: text.to_string(),
                    ts: None,
                    is_meta: false,
                }).collect(),
            );
        }
        let reg = AgentRegistry::new();
        reg.register_provider(Box::new(StubSearchProvider { sessions, messages }));
        reg
    }

    #[test]
    fn search_sessions_all_terms_must_match_in_same_session() {
        let sessions = vec![
            SessionMetadata {
                session_id: "s1".into(), provider: "stub".into(),
                project_path: None, title: None, summary: None,
                file_path: "f1".into(), created_ts: None, modified_ts: Some(2),
            },
            SessionMetadata {
                session_id: "s2".into(), provider: "stub".into(),
                project_path: None, title: None, summary: None,
                file_path: "f2".into(), created_ts: None, modified_ts: Some(1),
            },
        ];
        let reg = stub_registry(sessions, vec![
            ("f1", vec![("user", "webpack config crashed"), ("assistant", "try HMR")]),
            ("f2", vec![("user", "only webpack"), ("assistant", "no config talk here")]),
        ]);
        let hits = reg.search_sessions(&SessionSearchOpts {
            terms: vec!["webpack".into(), "config".into()],
            provider: None, project_path: None, limit: 10, max_snippets: 3,
        });
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].meta.session_id, "s1");
    }

    #[test]
    fn search_sessions_utf8_snippet_does_not_panic() {
        let sessions = vec![SessionMetadata {
            session_id: "s".into(), provider: "stub".into(),
            project_path: None, title: None, summary: None,
            file_path: "f".into(), created_ts: None, modified_ts: Some(1),
        }];
        let body = "Привет, пытаюсь починить webpack конфиг — всё валится на pnpm install у нас тут";
        let reg = stub_registry(sessions, vec![("f", vec![("user", body)])]);
        let hits = reg.search_sessions(&SessionSearchOpts {
            terms: vec!["webpack".into()],
            provider: None, project_path: None, limit: 5, max_snippets: 1,
        });
        assert_eq!(hits.len(), 1);
        let snippet = &hits[0].snippets[0].text;
        assert!(snippet.contains("webpack"));
        assert!(snippet.is_char_boundary(0));
        assert!(snippet.is_char_boundary(snippet.len()));
    }

    #[test]
    fn search_sessions_respects_provider_filter() {
        let sessions = vec![SessionMetadata {
            session_id: "s".into(), provider: "stub".into(),
            project_path: None, title: None, summary: None,
            file_path: "f".into(), created_ts: None, modified_ts: Some(1),
        }];
        let reg = stub_registry(sessions, vec![("f", vec![("user", "needle in haystack")])]);
        let hits_other = reg.search_sessions(&SessionSearchOpts {
            terms: vec!["needle".into()],
            provider: Some("nonexistent".into()),
            project_path: None, limit: 10, max_snippets: 3,
        });
        assert!(hits_other.is_empty());

        let hits_match = reg.search_sessions(&SessionSearchOpts {
            terms: vec!["needle".into()],
            provider: Some("stub".into()),
            project_path: None, limit: 10, max_snippets: 3,
        });
        assert_eq!(hits_match.len(), 1);
    }
}
