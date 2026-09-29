//! Session-content capability parity: the content source each runtime declares,
//! JS delegation to a native fallback, and OpenCode's per-session read from its
//! shared sqlite store through the sandboxed host API.

use std::sync::Arc;

use super::*;
use crate::agent_claude::provider::ClaudeProvider;
use crate::host_traits::PluginProviderRegistrar;
use crate::provider::{AgentProvider, SessionContentQuery, SessionContentSource};

struct ClaudeFallback;
impl PluginProviderRegistrar for ClaudeFallback {
    fn register_provider(&self, _: Box<dyn AgentProvider>) {}
    fn unregister_provider(&self, _: &str) -> bool {
        false
    }
    fn native_fallback(&self, _: &str) -> Option<Arc<dyn AgentProvider>> {
        Some(Arc::new(ClaudeProvider::new()))
    }
}

fn query(max_bytes: u64, hints: &[&str]) -> SessionContentQuery {
    SessionContentQuery {
        max_bytes,
        hint_terms: hints.iter().map(|h| h.to_string()).collect(),
    }
}

#[test]
fn session_content_source_parity() {
    if require_plugins().is_none() {
        return;
    }
    let native = ClaudeProvider::new();
    assert_eq!(
        native.session_content_source(),
        SessionContentSource::SessionFile
    );
    assert_eq!(
        load_js("claude").session_content_source(),
        native.session_content_source()
    );
    assert_eq!(
        load_js("codex").session_content_source(),
        SessionContentSource::SessionFile
    );
    assert_eq!(
        load_js("opencode").session_content_source(),
        SessionContentSource::SharedStore
    );

    let bare = load_inline_js("bare", "module.exports.default = {};");
    assert_eq!(bare.session_content_source(), SessionContentSource::None);
    assert!(bare
        .read_session_content("ses_x", &query(1024, &[]))
        .is_none());
    assert!(native
        .read_session_content("ses_x", &query(1024, &[]))
        .is_none());

    let mut delegating = load_inline_js("claude", "module.exports.default = {};");
    delegating.set_registrar(Arc::new(ClaudeFallback));
    assert_eq!(
        delegating.session_content_source(),
        native.session_content_source(),
        "a JS plugin without content exports inherits its native fallback's source"
    );
}

const PART_OLD: &str = r#"{"type":"text","text":"alpha needle older"}"#;
const PART_NEW: &str = r#"{"type":"text","text":"beta newest"}"#;
const PART_OTHER: &str = r#"{"type":"text","text":"needle in another session"}"#;

fn shared_store(path: &std::path::Path) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE session (id TEXT PRIMARY KEY, directory TEXT, title TEXT, time_created INTEGER, time_updated INTEGER);
         CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT);
         CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT);
         INSERT INTO message VALUES ('m1', 'ses_one', 1000, '{\"role\":\"user\"}');
         INSERT INTO message VALUES ('m2', 'ses_one', 2000, '{\"role\":\"assistant\"}');
         INSERT INTO message VALUES ('m3', 'ses_two', 3000, '{\"role\":\"user\"}');",
    )
    .unwrap();
    for (id, message, session, ts, data) in [
        ("p1", "m1", "ses_one", 1000, PART_OLD),
        ("p2", "m2", "ses_one", 2000, PART_NEW),
        ("p3", "m3", "ses_two", 3000, PART_OTHER),
    ] {
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?4, ?4, ?5)",
            rusqlite::params![id, message, session, ts, data],
        )
        .unwrap();
    }
}

fn texts(content: &crate::provider::SessionContent) -> Vec<(String, String)> {
    content
        .messages
        .iter()
        .map(|m| (m.role.clone(), m.text.clone()))
        .collect()
}

#[test]
fn opencode_reads_one_session_newest_first_within_budget() {
    if require_plugins().is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    shared_store(&db);
    let _env = EnvGuard::new(&[("OPENCODE_DB", db.to_str().unwrap())]);
    let j = load_js("opencode");

    let whole = j
        .read_session_content("ses_one", &query(1024 * 1024, &[]))
        .expect("readable store");
    assert!(!whole.truncated);
    assert_eq!(
        texts(&whole),
        vec![
            ("user".to_string(), "alpha needle older".to_string()),
            ("assistant".to_string(), "beta newest".to_string()),
        ],
        "only this session's parts, oldest first"
    );

    let hinted = j
        .read_session_content("ses_one", &query(1024 * 1024, &["needle"]))
        .expect("readable store");
    assert_eq!(
        texts(&hinted),
        vec![("user".to_string(), "alpha needle older".to_string())]
    );

    let newest_only = j
        .read_session_content("ses_one", &query(PART_NEW.len() as u64, &[]))
        .expect("readable store");
    assert!(
        newest_only.truncated,
        "older bytes past the budget went unscanned"
    );
    assert_eq!(
        texts(&newest_only),
        vec![("assistant".to_string(), "beta newest".to_string())]
    );

    assert!(j
        .read_session_content("not-a-session", &query(1024, &[]))
        .is_none());
}
