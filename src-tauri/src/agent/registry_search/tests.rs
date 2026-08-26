use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::*;
use crate::agent::provider::SessionMessage;

struct StubSearchProvider {
    sessions: Vec<SessionMetadata>,
    messages: HashMap<String, Vec<SessionMessage>>,
    parsed: Arc<AtomicUsize>,
    parsed_paths: Arc<parking_lot::Mutex<Vec<String>>>,
}

impl AgentProvider for StubSearchProvider {
    fn id(&self) -> &str {
        "stub"
    }
    fn display_name(&self) -> &str {
        "Stub"
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
    fn enumerate_sessions(&self) -> Vec<SessionMetadata> {
        self.sessions.clone()
    }
    fn parse_session_messages(&self, path: &std::path::Path) -> Vec<SessionMessage> {
        let key = path.to_str().unwrap_or("").to_string();
        self.parsed.fetch_add(1, Ordering::SeqCst);
        self.parsed_paths.lock().push(key.clone());
        self.messages.get(&key).cloned().unwrap_or_default()
    }
}

struct Probe {
    registry: AgentRegistry,
    parsed: Arc<AtomicUsize>,
    parsed_paths: Arc<parking_lot::Mutex<Vec<String>>>,
}

impl Probe {
    fn parse_calls(&self) -> usize {
        self.parsed.load(Ordering::SeqCst)
    }
    fn reset(&self) {
        self.parsed.store(0, Ordering::SeqCst);
        self.parsed_paths.lock().clear();
    }
    fn paths(&self) -> Vec<String> {
        self.parsed_paths.lock().clone()
    }
}

fn session(id: &str, file: &str, modified_ts: i64) -> SessionMetadata {
    SessionMetadata {
        session_id: id.into(),
        provider: "stub".into(),
        project_path: None,
        title: None,
        summary: None,
        file_path: file.into(),
        transcript_path: None,
        created_ts: None,
        modified_ts: Some(modified_ts),
    }
}

fn probe(sessions: Vec<SessionMetadata>, msgs: Vec<(&str, Vec<(&str, &str)>)>) -> Probe {
    let mut messages = HashMap::new();
    for (file, list) in msgs {
        messages.insert(
            file.to_string(),
            list.into_iter()
                .map(|(role, text)| SessionMessage {
                    role: role.to_string(),
                    text: text.to_string(),
                    ts: None,
                    is_meta: false,
                })
                .collect(),
        );
    }
    let parsed = Arc::new(AtomicUsize::new(0));
    let parsed_paths = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(StubSearchProvider {
        sessions,
        messages,
        parsed: parsed.clone(),
        parsed_paths: parsed_paths.clone(),
    }));
    Probe {
        registry,
        parsed,
        parsed_paths,
    }
}

fn stub_registry(
    sessions: Vec<SessionMetadata>,
    msgs: Vec<(&str, Vec<(&str, &str)>)>,
) -> AgentRegistry {
    probe(sessions, msgs).registry
}

fn opts(terms: &[&str], limit: usize) -> SessionSearchOpts {
    SessionSearchOpts {
        terms: terms.iter().map(|t| t.to_string()).collect(),
        provider: None,
        project_path: None,
        limit,
        max_snippets: 3,
        max_file_bytes: 0,
    }
}

/// The scan must stop at `limit` in recency order. Three sessions, strictly
/// decreasing `modified_ts`, only the NEWEST carries the term: a bounded scan
/// parses exactly one file, while the pre-fix implementation parsed every
/// session before truncating. The oldest-match half of the assertion proves the
/// early stop did not cost recall — a hit behind two misses is still returned.
#[test]
fn search_stops_reading_once_limit_is_satisfied() {
    let sessions = vec![
        session("newest", "f-new", 300),
        session("middle", "f-mid", 200),
        session("oldest", "f-old", 100),
    ];
    let p = probe(
        sessions.clone(),
        vec![
            ("f-new", vec![("user", "the needle is here")]),
            ("f-mid", vec![("user", "unrelated chatter")]),
            ("f-old", vec![("user", "also unrelated")]),
        ],
    );
    let hits = p.registry.search_sessions(&opts(&["needle"], 1));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].meta.session_id, "newest");
    assert_eq!(
        p.parse_calls(),
        1,
        "limit 1 satisfied by the most recent session must not parse the rest"
    );
    assert_eq!(p.paths(), vec!["f-new".to_string()]);

    // Same corpus, term only in the OLDEST session: recall is unchanged.
    let p = probe(
        sessions,
        vec![
            ("f-new", vec![("user", "unrelated chatter")]),
            ("f-mid", vec![("user", "also unrelated")]),
            ("f-old", vec![("user", "the needle is here")]),
        ],
    );
    let hits = p.registry.search_sessions(&opts(&["needle"], 1));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].meta.session_id, "oldest");
    assert_eq!(p.parse_calls(), 3, "a hit behind two misses is still found");
}

/// A session file larger than the cap is skipped without being handed to the
/// provider parser. Both fixtures contain the term and both are real files, so
/// the byte cap is the only thing that can distinguish them; the cap is injected
/// so the "oversized" fixture stays kilobytes, not gigabytes.
#[test]
fn oversized_session_file_is_skipped_not_loaded() {
    let dir = tempfile::tempdir().expect("tempdir");
    let big_path = dir.path().join("big.jsonl");
    let small_path = dir.path().join("small.jsonl");
    std::fs::write(&big_path, "needle ".repeat(1024)).expect("write big");
    std::fs::write(&small_path, "needle\n").expect("write small");

    let big = big_path.to_string_lossy().to_string();
    let small = small_path.to_string_lossy().to_string();
    assert!(
        std::fs::metadata(&big).unwrap().len() > 1024,
        "oversized fixture must exceed the injected cap"
    );
    assert!(
        std::fs::metadata(&small).unwrap().len() <= 1024,
        "small fixture must sit under the injected cap"
    );

    let p = probe(
        vec![session("big", &big, 300), session("small", &small, 200)],
        vec![
            (big.as_str(), vec![("user", "needle in the big file")]),
            (small.as_str(), vec![("user", "needle in the small file")]),
        ],
    );

    let hits = p.registry.search_sessions(&SessionSearchOpts {
        max_file_bytes: 1024,
        ..opts(&["needle"], 10)
    });
    assert_eq!(hits.len(), 1, "only the under-cap session may match");
    assert_eq!(hits[0].meta.session_id, "small");
    assert_eq!(
        p.paths(),
        vec![small.clone()],
        "oversized file never parsed"
    );

    // Without the cap both files match — proving the cap alone drove the skip.
    p.reset();
    let hits = p.registry.search_sessions(&opts(&["needle"], 10));
    assert_eq!(hits.len(), 2);
    assert_eq!(p.parse_calls(), 2);
}

#[test]
fn search_sessions_all_terms_must_match_in_same_session() {
    let sessions = vec![session("s1", "f1", 2), session("s2", "f2", 1)];
    let reg = stub_registry(
        sessions,
        vec![
            (
                "f1",
                vec![("user", "webpack config crashed"), ("assistant", "try HMR")],
            ),
            (
                "f2",
                vec![
                    ("user", "only webpack"),
                    ("assistant", "no config talk here"),
                ],
            ),
        ],
    );
    let hits = reg.search_sessions(&opts(&["webpack", "config"], 10));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].meta.session_id, "s1");
}

#[test]
fn search_sessions_utf8_snippet_does_not_panic() {
    let body = "Привет, пытаюсь починить webpack конфиг — всё валится на pnpm install у нас тут";
    let reg = stub_registry(
        vec![session("s", "f", 1)],
        vec![("f", vec![("user", body)])],
    );
    let hits = reg.search_sessions(&SessionSearchOpts {
        max_snippets: 1,
        ..opts(&["webpack"], 5)
    });
    assert_eq!(hits.len(), 1);
    let snippet = &hits[0].snippets[0].text;
    assert!(snippet.contains("webpack"));
    assert!(snippet.is_char_boundary(0));
    assert!(snippet.is_char_boundary(snippet.len()));
}

#[test]
fn search_sessions_respects_provider_filter() {
    let reg = stub_registry(
        vec![session("s", "f", 1)],
        vec![("f", vec![("user", "needle in haystack")])],
    );
    let hits_other = reg.search_sessions(&SessionSearchOpts {
        provider: Some("nonexistent".into()),
        ..opts(&["needle"], 10)
    });
    assert!(hits_other.is_empty());

    let hits_match = reg.search_sessions(&SessionSearchOpts {
        provider: Some("stub".into()),
        ..opts(&["needle"], 10)
    });
    assert_eq!(hits_match.len(), 1);
}

#[test]
fn search_sessions_returns_hits_newest_first() {
    let reg = stub_registry(
        vec![
            session("old", "f-old", 100),
            session("new", "f-new", 300),
            session("mid", "f-mid", 200),
        ],
        vec![
            ("f-old", vec![("user", "needle")]),
            ("f-new", vec![("user", "needle")]),
            ("f-mid", vec![("user", "needle")]),
        ],
    );
    let ids: Vec<_> = reg
        .search_sessions(&opts(&["needle"], 10))
        .into_iter()
        .map(|h| h.meta.session_id)
        .collect();
    assert_eq!(ids, vec!["new", "mid", "old"]);
}

// UTF-8 safety: snippet windowing must never slice mid-codepoint.
#[test]
fn floor_char_boundary_handles_utf8() {
    let s = "Привет мир";
    assert_eq!(floor_char_boundary(s, 0), 0);
    assert_eq!(floor_char_boundary(s, s.len()), s.len());
    for i in 1..s.len() {
        let b = floor_char_boundary(s, i);
        assert!(s.is_char_boundary(b));
        assert!(b <= i);
    }
}
