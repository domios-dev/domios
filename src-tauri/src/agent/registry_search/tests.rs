use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::*;
use crate::agent::provider::{SessionContentSource, SessionMessage};

struct StubSearchProvider {
    source: SessionContentSource,
    sessions: Vec<SessionMetadata>,
    messages: HashMap<String, Vec<SessionMessage>>,
    parsed: Arc<AtomicUsize>,
    parsed_paths: Arc<parking_lot::Mutex<Vec<String>>>,
    described: Arc<AtomicUsize>,
    described_paths: Arc<parking_lot::Mutex<Vec<String>>>,
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
    fn describe_session_for_search(&self, session: &SessionMetadata) -> SessionMetadata {
        self.described.fetch_add(1, Ordering::SeqCst);
        self.described_paths.lock().push(session.file_path.clone());
        SessionMetadata {
            project_path: Some("/work/project".into()),
            ..session.clone()
        }
    }
    fn session_content_source(&self) -> SessionContentSource {
        self.source
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
    described: Arc<AtomicUsize>,
    described_paths: Arc<parking_lot::Mutex<Vec<String>>>,
}

impl Probe {
    fn parse_calls(&self) -> usize {
        self.parsed.load(Ordering::SeqCst)
    }
    fn reset(&self) {
        self.parsed.store(0, Ordering::SeqCst);
        self.parsed_paths.lock().clear();
        self.described.store(0, Ordering::SeqCst);
        self.described_paths.lock().clear();
    }
    fn paths(&self) -> Vec<String> {
        self.parsed_paths.lock().clone()
    }
    fn describe_calls(&self) -> usize {
        self.described.load(Ordering::SeqCst)
    }
    fn described_paths(&self) -> Vec<String> {
        self.described_paths.lock().clone()
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
    probe_with(SessionContentSource::SessionFile, sessions, msgs)
}

fn probe_with(
    source: SessionContentSource,
    sessions: Vec<SessionMetadata>,
    msgs: Vec<(&str, Vec<(&str, &str)>)>,
) -> Probe {
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
    let described = Arc::new(AtomicUsize::new(0));
    let described_paths = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(StubSearchProvider {
        source,
        sessions,
        messages,
        parsed: parsed.clone(),
        parsed_paths: parsed_paths.clone(),
        described: described.clone(),
        described_paths: described_paths.clone(),
    }));
    Probe {
        registry,
        parsed,
        parsed_paths,
        described,
        described_paths,
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
        match_mode: SessionMatchMode::AllTerms,
        provider: None,
        project_path: None,
        limit,
        max_snippets: 3,
        max_file_bytes: 0,
    }
}

#[test]
fn session_search_opts_default_to_all_terms() {
    assert_eq!(
        SessionSearchOpts::default().match_mode,
        SessionMatchMode::AllTerms
    );
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
    let hits = p
        .registry
        .search_sessions(&opts(&["needle"], 1))
        .unwrap()
        .hits;
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
    let hits = p
        .registry
        .search_sessions(&opts(&["needle"], 1))
        .unwrap()
        .hits;
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

    let result = p
        .registry
        .search_sessions(&SessionSearchOpts {
            max_file_bytes: 1024,
            ..opts(&["needle"], 10)
        })
        .unwrap();
    assert_eq!(
        result
            .warnings
            .iter()
            .map(|w| (w.provider.as_str(), w.reason, w.sessions))
            .collect::<Vec<_>>(),
        vec![("stub", SessionSkipReason::OverByteCap, 1)],
        "the skipped file is reported, never a silent miss"
    );
    let hits = result.hits;
    assert_eq!(hits.len(), 1, "only the under-cap session may match");
    assert_eq!(hits[0].meta.session_id, "small");
    assert_eq!(
        p.paths(),
        vec![small.clone()],
        "oversized file never parsed"
    );

    // Without the cap both files match — proving the cap alone drove the skip.
    p.reset();
    let result = p.registry.search_sessions(&opts(&["needle"], 10)).unwrap();
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    assert_eq!(result.hits.len(), 2);
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
    let hits = reg
        .search_sessions(&opts(&["webpack", "config"], 10))
        .unwrap()
        .hits;
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].meta.session_id, "s1");
}

#[test]
fn registry_search_any_term_matches_subset() {
    let reg = stub_registry(
        vec![
            session("webpack", "f-webpack", 2),
            session("memory", "f-memory", 1),
        ],
        vec![
            ("f-webpack", vec![("user", "webpack config")]),
            ("f-memory", vec![("user", "memory lookup")]),
        ],
    );
    let result = reg
        .search_sessions(&SessionSearchOpts {
            match_mode: SessionMatchMode::AnyTermRanked,
            ..opts(&["the", "webpack", "memory", "missing"], 10)
        })
        .unwrap();

    let ids: Vec<_> = result
        .hits
        .iter()
        .map(|hit| hit.meta.session_id.as_str())
        .collect();
    assert_eq!(ids, vec!["webpack", "memory"]);
}

#[test]
fn registry_search_any_term_drops_stop_words_before_scanning() {
    let p = probe(
        vec![session("meeting", "f-meeting", 1)],
        vec![("f-meeting", vec![("user", "the meeting notes")])],
    );
    let (result, stats) = p
        .registry
        .search_sessions_with_stats(&SessionSearchOpts {
            match_mode: SessionMatchMode::AnyTermRanked,
            ..opts(&["the"], 10)
        })
        .unwrap();

    assert!(result.hits.is_empty());
    assert_eq!(stats.scanned, 0);
    assert_eq!(p.parse_calls(), 0);
}

#[test]
fn registry_search_any_term_ranks_by_bm25_then_recency() {
    let reg = stub_registry(
        vec![
            session("bm25", "f-bm25", 1),
            session("recent", "f-recent", 30),
            session("older", "f-older", 10),
            session("common", "f-common", 100),
        ],
        vec![
            ("f-bm25", vec![("user", "alpha beta alpha beta")]),
            ("f-recent", vec![("user", "alpha beta")]),
            ("f-older", vec![("user", "alpha beta")]),
            ("f-common", vec![("user", "alpha alpha alpha alpha")]),
        ],
    );
    let result = reg
        .search_sessions(&SessionSearchOpts {
            match_mode: SessionMatchMode::AnyTermRanked,
            ..opts(&["alpha", "beta"], 10)
        })
        .unwrap();

    let ids: Vec<_> = result
        .hits
        .iter()
        .map(|hit| hit.meta.session_id.as_str())
        .collect();
    assert_eq!(ids, vec!["bm25", "recent", "older", "common"]);
}

#[test]
fn registry_search_any_term_respects_scan_budget_and_limit() {
    let total = ANY_TERM_CANDIDATE_BUDGET_PER_PROVIDER + 8;
    let files: Vec<String> = (0..total).map(|index| format!("f-{index}")).collect();
    let bodies: Vec<String> = (0..total)
        .map(|index| {
            if index == ANY_TERM_CANDIDATE_BUDGET_PER_PROVIDER - 1 {
                "alpha beta alpha beta alpha beta".into()
            } else {
                "alpha beta".into()
            }
        })
        .collect();
    let sessions = files
        .iter()
        .enumerate()
        .map(|(index, file)| session(&format!("s-{index}"), file, (total - index) as i64))
        .collect();
    let messages = files
        .iter()
        .zip(&bodies)
        .map(|(file, body)| (file.as_str(), vec![("user", body.as_str())]))
        .collect();
    let p = probe(sessions, messages);
    let (result, stats) = p
        .registry
        .search_sessions_with_stats(&SessionSearchOpts {
            match_mode: SessionMatchMode::AnyTermRanked,
            ..opts(&["alpha", "beta"], 1)
        })
        .unwrap();

    assert_eq!(stats.scanned, ANY_TERM_CANDIDATE_BUDGET_PER_PROVIDER);
    assert_eq!(stats.matched, ANY_TERM_CANDIDATE_BUDGET_PER_PROVIDER);
    assert_eq!(p.parse_calls(), ANY_TERM_CANDIDATE_BUDGET_PER_PROVIDER);
    assert_eq!(result.hits.len(), 1);
    assert_eq!(
        result.hits[0].meta.session_id,
        format!("s-{}", ANY_TERM_CANDIDATE_BUDGET_PER_PROVIDER - 1)
    );
}

#[test]
fn search_sessions_utf8_snippet_does_not_panic() {
    let body = "Привет, пытаюсь починить webpack конфиг — всё валится на pnpm install у нас тут";
    let reg = stub_registry(
        vec![session("s", "f", 1)],
        vec![("f", vec![("user", body)])],
    );
    let hits = reg
        .search_sessions(&SessionSearchOpts {
            max_snippets: 1,
            ..opts(&["webpack"], 5)
        })
        .unwrap()
        .hits;
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
    let unknown = reg.search_sessions(&SessionSearchOpts {
        provider: Some("nonexistent".into()),
        ..opts(&["needle"], 10)
    });
    assert_eq!(
        unknown.unwrap_err(),
        SessionSearchError::UnknownProvider("nonexistent".into()),
        "an unknown provider is an error, never an empty match"
    );

    let hits_match = reg
        .search_sessions(&SessionSearchOpts {
            provider: Some("stub".into()),
            ..opts(&["needle"], 10)
        })
        .unwrap()
        .hits;
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
        .unwrap()
        .hits
        .into_iter()
        .map(|h| h.meta.session_id)
        .collect();
    assert_eq!(ids, vec!["new", "mid", "old"]);
}

/// Raw-byte prefilter: a real file lacking a hint-safe term is never handed to
/// the parser, while a term the prefilter cannot vouch for (non-ASCII) still
/// parses every file, so the prefilter only ever removes certain misses.
#[test]
fn raw_prefilter_parses_only_files_holding_every_term() {
    let dir = tempfile::tempdir().expect("tempdir");
    let with_path = dir.path().join("with.jsonl");
    let without_path = dir.path().join("without.jsonl");
    std::fs::write(&with_path, "{\"text\":\"The Needle and grüße\"}\n").expect("write");
    std::fs::write(&without_path, "{\"text\":\"haystack grüße\"}\n").expect("write");
    let with = with_path.to_string_lossy().to_string();
    let without = without_path.to_string_lossy().to_string();
    let p = probe(
        vec![
            session("with", &with, 200),
            session("without", &without, 300),
        ],
        vec![
            (with.as_str(), vec![("user", "The Needle and grüße")]),
            (without.as_str(), vec![("user", "haystack grüße")]),
        ],
    );

    let hits = p
        .registry
        .search_sessions(&opts(&["NEEDLE"], 10))
        .unwrap()
        .hits;
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].meta.session_id, "with");
    assert_eq!(
        p.paths(),
        vec![with.clone()],
        "the file without the term is never parsed"
    );

    p.reset();
    let hits = p
        .registry
        .search_sessions(&opts(&["grüße"], 10))
        .unwrap()
        .hits;
    assert_eq!(hits.len(), 2);
    assert_eq!(p.parse_calls(), 2, "a non-hint term cannot skip the parse");
}

#[test]
fn search_describes_only_matching_files_after_raw_prefilter() {
    let dir = tempfile::tempdir().expect("tempdir");
    let miss_path = dir.path().join("miss.jsonl");
    let false_positive_path = dir.path().join("false-positive.jsonl");
    let hit_path = dir.path().join("hit.jsonl");
    std::fs::write(&miss_path, "unrelated\n").expect("write miss");
    std::fs::write(&false_positive_path, "needle in metadata only\n")
        .expect("write false positive");
    std::fs::write(&hit_path, "needle in conversation\n").expect("write hit");
    let miss = miss_path.to_string_lossy().to_string();
    let false_positive = false_positive_path.to_string_lossy().to_string();
    let hit = hit_path.to_string_lossy().to_string();
    let p = probe(
        vec![
            session("miss", &miss, 300),
            session("false-positive", &false_positive, 200),
            session("hit", &hit, 100),
        ],
        vec![
            (&miss, vec![("user", "unrelated")]),
            (
                &false_positive,
                vec![("user", "no match in parsed content")],
            ),
            (&hit, vec![("user", "needle in parsed content")]),
        ],
    );

    let result = p.registry.search_sessions(&opts(&["needle"], 10)).unwrap();

    assert_eq!(result.hits.len(), 1);
    assert_eq!(
        result.hits[0].meta.project_path.as_deref(),
        Some("/work/project")
    );
    assert_eq!(p.describe_calls(), 1);
    assert_eq!(p.described_paths(), vec![hit]);
}

#[test]
fn project_filter_resolves_unknown_paths_after_raw_prefilter() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("candidate.jsonl");
    std::fs::write(&path, "needle\n").expect("write candidate");
    let file = path.to_string_lossy().to_string();
    let p = probe(
        vec![session("candidate", &file, 1)],
        vec![(&file, vec![("user", "needle in parsed content")])],
    );

    let matching = p
        .registry
        .search_sessions(&SessionSearchOpts {
            project_path: Some("/work/project".into()),
            ..opts(&["needle"], 10)
        })
        .unwrap();
    assert_eq!(matching.hits.len(), 1);
    assert_eq!(p.parse_calls(), 1);
    assert_eq!(p.describe_calls(), 1);

    p.reset();
    let other_project = p
        .registry
        .search_sessions(&SessionSearchOpts {
            project_path: Some("/work/other".into()),
            ..opts(&["needle"], 10)
        })
        .unwrap();
    assert!(other_project.hits.is_empty());
    assert_eq!(p.parse_calls(), 0);
    assert_eq!(p.describe_calls(), 1);
}

#[test]
fn provider_without_content_source_is_reported_not_silently_empty() {
    let p = probe_with(
        SessionContentSource::None,
        vec![session("a", "f-a", 2), session("b", "f-b", 1)],
        vec![("f-a", vec![("user", "needle")])],
    );
    let result = p.registry.search_sessions(&opts(&["needle"], 10)).unwrap();
    assert!(result.hits.is_empty());
    assert_eq!(
        p.parse_calls(),
        0,
        "no content source means nothing is read"
    );
    assert_eq!(result.warnings.len(), 1);
    assert_eq!(result.warnings[0].provider, "stub");
    assert_eq!(
        result.warnings[0].reason,
        SessionSkipReason::NoContentSource
    );
    assert_eq!(result.warnings[0].sessions, 2);
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
