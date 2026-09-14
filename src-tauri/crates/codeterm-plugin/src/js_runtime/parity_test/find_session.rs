//! Exact session lookup by id on shared fixtures: native Claude and the JS path
//! agree; the Codex plugin answers from its own thread row, never session history.

use super::*;
use crate::agent_claude::provider::{enumerate_sessions_under, find_session_under};

const CLAUDE_SID: &str = "aaaa1111-find-4aaa-8aaa-aaaaaaaaaaaa";
const CLAUDE_SIBLING: &str = "bbbb2222-find-4bbb-8bbb-bbbbbbbbbbbb";
const CODEX_INDEXED: &str = "11111111-1111-1111-1111-111111111111";
const CODEX_UNINDEXED: &str = "33333333-3333-3333-3333-333333333333";

fn claude_js_rooted_at(projects_root: &std::path::Path) -> JsProvider {
    let dir = workspace_plugins_root().join("provider-claude");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("plugin.json")).unwrap()).unwrap();
    manifest["sessions"]["rootDir"] = serde_json::json!(projects_root.to_string_lossy());
    let manifest_json = manifest.to_string();
    let parsed: super::super::PluginManifest = serde_json::from_str(&manifest_json).unwrap();
    let source = std::fs::read_to_string(dir.join("plugin.js")).unwrap();
    let vm = super::super::PluginVm::from_source_with("claude", source, Some(manifest_json))
        .expect("claude vm");
    JsProvider::new(parsed, std::sync::Arc::new(vm))
}

fn json_of(meta: &crate::provider::SessionMetadata) -> serde_json::Value {
    serde_json::to_value(meta).unwrap()
}

#[test]
fn claude_find_session_native_js_parity() {
    if require_plugins().is_none() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let evidence = tmp.path().join("evidence");
    let _env = EnvGuard::new(&[(
        crate::session_evidence::EVIDENCE_DIR_ENV,
        evidence.to_str().unwrap(),
    )]);
    let root = tmp.path().join("projects");
    let sibling_dir = root.join("-work-sibling");
    let owned_dir = root.join("-work-owned");
    std::fs::create_dir_all(&sibling_dir).unwrap();
    std::fs::create_dir_all(&owned_dir).unwrap();
    std::fs::write(
        sibling_dir.join(format!("{CLAUDE_SIBLING}.jsonl")),
        "{\"type\":\"user\",\"cwd\":\"/work/sibling\"}\n",
    )
    .unwrap();
    std::fs::write(
        owned_dir.join(format!("{CLAUDE_SID}.jsonl")),
        concat!(
            "{\"type\":\"permission-mode\",\"permissionMode\":\"default\"}\n",
            "{\"type\":\"user\",\"cwd\":\"/work/owned\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        ),
    )
    .unwrap();

    let js = claude_js_rooted_at(&root);
    let native_found = find_session_under(&root, CLAUDE_SID).expect("native resolves the id");
    let js_found = js.find_session(CLAUDE_SID).expect("JS resolves the id");

    assert_eq!(json_of(&native_found), json_of(&js_found));
    assert_eq!(native_found.provider, "claude");
    assert_eq!(native_found.project_path.as_deref(), Some("/work/owned"));
    assert!(native_found.file_path.contains("-work-owned"));

    let native_listed = enumerate_sessions_under(&root)
        .into_iter()
        .find(|s| s.session_id == CLAUDE_SID)
        .expect("native enumerates the fixture");
    let js_listed = js
        .enumerate_sessions()
        .into_iter()
        .find(|s| s.session_id == CLAUDE_SID)
        .expect("JS enumerates the fixture");
    assert_eq!(json_of(&native_found), json_of(&native_listed));
    assert_eq!(json_of(&js_found), json_of(&js_listed));

    let pathlike = format!("../-work-owned/{CLAUDE_SID}");
    for unowned in [
        "cccc3333-none-4ccc-8ccc-cccccccccccc",
        pathlike.as_str(),
        "",
    ] {
        assert!(find_session_under(&root, unowned).is_none(), "{unowned}");
        assert!(js.find_session(unowned).is_none(), "{unowned}");
    }
}

#[test]
fn codex_find_session_reads_only_its_own_thread_row() {
    if require_plugins().is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let index = dir.path().join("session_index.jsonl");
    let state_db = dir.path().join("state_5.sqlite");
    let proj = dir.path().join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    let proj = proj.to_str().unwrap().to_string();
    std::fs::write(
        &index,
        format!(
            "{}\n",
            serde_json::json!({
                "id": CODEX_INDEXED,
                "thread_name": "Renamed thread",
                "updated_at": "2026-09-01T10:00:00Z"
            })
        ),
    )
    .unwrap();
    let db = rusqlite::Connection::open(&state_db).unwrap();
    db.execute_batch(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT, created_at INTEGER, \
         updated_at INTEGER, cwd TEXT, title TEXT, model TEXT, name TEXT);",
    )
    .unwrap();
    db.execute(
        "INSERT INTO threads VALUES (?1, NULL, 1788256800, 1788260400, ?2, 'raw first prompt', 'gpt-5.6-sol', 'Renamed thread')",
        rusqlite::params![CODEX_INDEXED, proj],
    )
    .unwrap();
    db.execute(
        "INSERT INTO threads VALUES (?1, NULL, 1788256900, 1788260500, ?2, 'unindexed first prompt', NULL, NULL)",
        rusqlite::params![CODEX_UNINDEXED, proj],
    )
    .unwrap();
    drop(db);

    let _env = EnvGuard::new(&[
        ("CODETERM_CODEX_SESSION_INDEX_PATH", index.to_str().unwrap()),
        ("CODETERM_CODEX_STATE_DB_PATH", state_db.to_str().unwrap()),
    ]);
    let j = load_js("codex");

    let listed = j
        .enumerate_sessions()
        .into_iter()
        .find(|s| s.session_id == CODEX_INDEXED)
        .expect("indexed thread is enumerated");
    let found = j
        .find_session(CODEX_INDEXED)
        .expect("thread row owns the id");
    assert_eq!(found.session_id, listed.session_id);
    assert_eq!(found.provider, "codex");
    assert_eq!(found.provider, listed.provider);
    assert_eq!(found.project_path.as_deref(), Some(proj.as_str()));
    assert_eq!(found.project_path, listed.project_path);
    assert_eq!(found.file_path, listed.file_path);
    assert_eq!(found.title, listed.title);
    assert_eq!(found.created_ts, Some(1788256800));
    assert_eq!(found.modified_ts, Some(1788260400));

    let unindexed = j
        .find_session(CODEX_UNINDEXED)
        .expect("a thread row owns the id before the index names it");
    assert_eq!(unindexed.title.as_deref(), Some("unindexed first prompt"));
    assert_eq!(unindexed.project_path.as_deref(), Some(proj.as_str()));

    assert!(j
        .find_session("44444444-4444-4444-4444-444444444444")
        .is_none());

    std::fs::remove_file(&index).unwrap();
    let without_index = j
        .find_session(CODEX_INDEXED)
        .expect("lookup does not depend on the session index");
    assert_eq!(without_index.project_path, found.project_path);
}
