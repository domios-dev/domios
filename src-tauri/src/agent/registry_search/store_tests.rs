use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;

use super::*;
use crate::agent::provider::{SessionContent, SessionContentQuery, SessionContentSource};

type QueryLog = Arc<Mutex<Vec<(String, SessionContentQuery)>>>;

/// Shared-store provider: every session points at one store file, and content
/// comes only through `read_session_content`.
struct StoreStub {
    id: String,
    store_path: String,
    sessions: Vec<(String, i64)>,
    contents: HashMap<String, Option<SessionContent>>,
    queries: QueryLog,
}

impl AgentProvider for StoreStub {
    fn id(&self) -> &str {
        &self.id
    }
    fn display_name(&self) -> &str {
        "Store"
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
    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
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
        self.sessions
            .iter()
            .map(|(id, modified)| SessionMetadata {
                session_id: id.clone(),
                provider: self.id.clone(),
                project_path: None,
                title: None,
                summary: None,
                file_path: self.store_path.clone(),
                transcript_path: None,
                created_ts: None,
                modified_ts: Some(*modified),
            })
            .collect()
    }
    fn session_content_source(&self) -> SessionContentSource {
        SessionContentSource::SharedStore
    }
    fn parse_session_messages(&self, _: &Path) -> Vec<SessionMessage> {
        panic!("a shared store is never parsed as a session file");
    }
    fn read_session_content(
        &self,
        session_id: &str,
        query: &SessionContentQuery,
    ) -> Option<SessionContent> {
        self.queries
            .lock()
            .push((session_id.to_string(), query.clone()));
        self.contents.get(session_id).cloned().flatten()
    }
}

fn content(texts: &[&str], truncated: bool) -> Option<SessionContent> {
    Some(SessionContent {
        messages: texts
            .iter()
            .map(|text| SessionMessage {
                role: "user".into(),
                text: text.to_string(),
                ts: None,
                is_meta: false,
            })
            .collect(),
        truncated,
    })
}

fn store(
    id: &str,
    store_path: &str,
    sessions: &[(&str, i64, Option<SessionContent>)],
) -> (StoreStub, QueryLog) {
    let queries = QueryLog::default();
    let stub = StoreStub {
        id: id.into(),
        store_path: store_path.into(),
        sessions: sessions
            .iter()
            .map(|(s, m, _)| (s.to_string(), *m))
            .collect(),
        contents: sessions
            .iter()
            .map(|(s, _, c)| (s.to_string(), c.clone()))
            .collect(),
        queries: queries.clone(),
    };
    (stub, queries)
}

fn opts(terms: &[&str], limit: usize) -> SessionSearchOpts {
    SessionSearchOpts {
        terms: terms.iter().map(|t| t.to_string()).collect(),
        limit,
        max_snippets: 3,
        ..Default::default()
    }
}

/// The shared store file is far over the byte cap, yet its sessions are read
/// through the capability with the cap as the per-session budget; core never
/// stats or opens the store. Recency order and the early exit hold.
#[test]
fn shared_store_is_read_per_session_within_the_byte_cap() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store_path = dir.path().join("shared.db");
    std::fs::write(&store_path, vec![b'x'; 4096]).expect("write store");
    let (stub, queries) = store(
        "store",
        &store_path.to_string_lossy(),
        &[
            (
                "old",
                100,
                content(&["Needle kept in the oldest session"], false),
            ),
            (
                "new",
                300,
                content(&["needle in the newest session"], false),
            ),
            ("mid", 200, content(&["nothing here"], false)),
        ],
    );
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(stub));

    let result = registry
        .search_sessions(&SessionSearchOpts {
            max_file_bytes: 1024,
            ..opts(&["NEEDLE", "grüße"], 1)
        })
        .unwrap();
    assert!(result.hits.is_empty(), "grüße appears nowhere");
    let hints: Vec<Vec<String>> = queries
        .lock()
        .iter()
        .map(|(_, q)| q.hint_terms.clone())
        .collect();
    assert_eq!(
        hints,
        vec![vec!["needle".to_string()]; 3],
        "only escape-free ASCII terms become store hints"
    );

    queries.lock().clear();
    let result = registry
        .search_sessions(&SessionSearchOpts {
            max_file_bytes: 1024,
            ..opts(&["NEEDLE"], 1)
        })
        .unwrap();
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].meta.session_id, "new");
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    let queries = queries.lock().clone();
    assert_eq!(
        queries,
        vec![(
            "new".to_string(),
            SessionContentQuery {
                max_bytes: 1024,
                hint_terms: vec!["needle".into()],
            }
        )],
        "limit 1 met by the newest session reads nothing older"
    );
}

#[test]
fn sessions_search_reports_skipped_store() {
    let (stub, _) = store(
        "store",
        "/nonexistent/shared.db",
        &[
            ("gone", 300, None),
            ("partial", 200, content(&["unrelated"], true)),
            ("whole", 100, content(&["unrelated"], false)),
        ],
    );
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(stub));

    let result = registry.search_sessions(&opts(&["needle"], 5)).unwrap();
    assert!(result.hits.is_empty());
    let warnings: Vec<_> = result
        .warnings
        .iter()
        .map(|w| (w.provider.as_str(), w.reason, w.sessions))
        .collect();
    assert_eq!(
        warnings,
        vec![
            ("store", SessionSkipReason::Unreadable, 1),
            ("store", SessionSkipReason::ContentTruncated, 1),
        ]
    );
    assert!(result.warnings.iter().all(|w| w.message.contains("store")));

    let wire = serde_json::to_value(&result).expect("serialize");
    assert_eq!(wire["hits"], serde_json::json!([]));
    assert_eq!(wire["warnings"][0]["provider"], "store");
    assert_eq!(wire["warnings"][0]["reason"], "unreadable");
    assert_eq!(wire["warnings"][1]["reason"], "content_truncated");
}

/// Providers scan in parallel, each stopping at `limit`; the merge keeps the
/// newest `limit` across all of them.
#[test]
fn providers_merge_to_the_newest_limit_hits() {
    let (a, _) = store(
        "a",
        "/a.db",
        &[
            ("a1", 500, content(&["needle"], false)),
            ("a2", 200, content(&["needle"], false)),
        ],
    );
    let (b, _) = store(
        "b",
        "/b.db",
        &[
            ("b1", 400, content(&["needle"], false)),
            ("b2", 300, content(&["needle"], false)),
        ],
    );
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(a));
    registry.register_provider(Box::new(b));

    let ids: Vec<String> = registry
        .search_sessions(&opts(&["needle"], 3))
        .unwrap()
        .hits
        .into_iter()
        .map(|h| h.meta.session_id)
        .collect();
    assert_eq!(ids, vec!["a1", "b1", "b2"]);
}

fn plugins_root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/..")).join("plugins")
}

fn opencode_registry() -> AgentRegistry {
    let plugin_js =
        std::fs::read_to_string(plugins_root().join("provider-opencode").join("plugin.js"))
            .expect("read bundled opencode plugin.js");
    let manifest_json = r#"{
        "id": "opencode",
        "version": "0.1.0",
        "displayName": "opencode",
        "spawn": {
            "readinessStrategy": {"kind": "fixed_delay", "durationMs": 0},
            "taskDelivery": "pty_input",
            "verifySubmit": {"kind": "none"}
        }
    }"#;
    let manifest: crate::plugin::js_runtime::PluginManifest =
        serde_json::from_str(manifest_json).expect("parse manifest");
    let vm = crate::plugin::js_runtime::PluginVm::from_source_with(
        "opencode",
        plugin_js,
        Some(manifest_json.to_string()),
    )
    .expect("init opencode VM");
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(crate::plugin::js_runtime::JsProvider::new(
        manifest,
        Arc::new(vm),
    )));
    registry
}

fn opencode_store(path: &Path) {
    let conn = rusqlite::Connection::open(path).expect("open fixture store");
    conn.execute_batch(
        "CREATE TABLE session (id TEXT PRIMARY KEY, directory TEXT, title TEXT, time_created INTEGER, time_updated INTEGER);
         CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT);
         CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT);",
    )
    .expect("schema");
    let rows: [(&str, &str, i64, &str, &str); 3] = [
        (
            "ses_alpha",
            "m_alpha",
            1_700_000_300_000,
            "p_alpha",
            r#"{"type":"text","text":"rolled out the Zebrafirmware build"}"#,
        ),
        (
            "ses_alpha",
            "m_alpha",
            1_700_000_300_001,
            "p_alpha_tool",
            r#"{"type":"tool","tool":"bash","state":{"output":"zebrafirmware"}}"#,
        ),
        (
            "ses_beta",
            "m_beta",
            1_700_000_200_000,
            "p_beta",
            r#"{"type":"text","text":"zebrafirmware mentioned only by the system","synthetic":true}"#,
        ),
    ];
    for (session, message, ts, part, data) in rows {
        conn.execute(
            "INSERT OR IGNORE INTO session (id, directory, title, time_created, time_updated) VALUES (?1, '/work/proj', NULL, ?2, ?2)",
            rusqlite::params![session, ts],
        )
        .expect("session row");
        conn.execute(
            "INSERT OR IGNORE INTO message (id, session_id, time_created, data) VALUES (?1, ?2, ?3, '{\"role\":\"user\"}')",
            rusqlite::params![message, session, ts],
        )
        .expect("message row");
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?4, ?4, ?5)",
            rusqlite::params![part, message, session, ts, data],
        )
        .expect("part row");
    }
}

/// The real OpenCode plugin against a fixture sqlite store holding two
/// sessions. The store file exceeds the injected cap, as the shared
/// opencode.db always does; the term still resolves to its own session only.
#[test]
fn sessions_search_uses_provider_session_content() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("opencode.db");
    opencode_store(&db);
    let mut env = codeterm_util::test_env::EnvGuard::lock();
    env.set("OPENCODE_DB", &db);
    let registry = opencode_registry();
    let cap = 1024;
    assert!(std::fs::metadata(&db).expect("store").len() > cap);

    let result = registry
        .search_sessions(&SessionSearchOpts {
            provider: Some("opencode".into()),
            max_file_bytes: cap,
            ..opts(&["zebrafirmware"], 2)
        })
        .unwrap();
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    let ids: Vec<&str> = result
        .hits
        .iter()
        .map(|h| h.meta.session_id.as_str())
        .collect();
    assert_eq!(ids, vec!["ses_alpha"], "synthetic text never matches");
    assert_eq!(result.hits[0].meta.provider, "opencode");
    assert_eq!(
        result.hits[0].match_count, 1,
        "tool output is not message text"
    );
    assert!(result.hits[0].snippets[0]
        .text
        .to_ascii_lowercase()
        .contains("zebrafirmware"));
    drop(env);
}
