//! OpenCode sqlite scans stay bounded on a large store: usage aggregates in SQL
//! and no scan parses a message row past the plugin's byte guard.

use super::*;

const SES_ID: &str = "ses_big";

/// 3000 assistant rows plus one newest user row that is both oversized and
/// malformed, so any scan that parses it fails the whole query.
fn build_large_opencode_db(path: &std::path::Path) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE session (id TEXT PRIMARY KEY, directory TEXT, time_created INTEGER);
         CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT);
         CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT);
         INSERT INTO session VALUES ('ses_big', '/work/proj', 1);
         WITH RECURSIVE n(k) AS (SELECT 1 UNION ALL SELECT k + 1 FROM n WHERE k < 3000)
         INSERT INTO message SELECT 'msg_' || k, 'ses_big', k, k, json_object(
             'role', 'assistant',
             'providerID', CASE WHEN k % 3 = 0 THEN 'beta' ELSE 'alpha' END,
             'modelID', 'fixture-model',
             'tokens', json_object('input', 2, 'output', 1, 'cache', json_object('read', 0, 'write', 0)),
             'cost', 0) FROM n;
         INSERT INTO message VALUES ('msg_huge', 'ses_big', 5000, 5000,
             '{\"role\":\"user\",\"pad\":\"' || printf('%.2097152c', 'x'));
         INSERT INTO part VALUES ('p1', 'msg_1', 'ses_big', 10, 10, '{\"type\":\"text\",\"text\":\"assistant reply\"}');
         INSERT INTO part VALUES ('p2', 'msg_huge', 'ses_big', 5000, 5000, '{\"type\":\"text\",\"text\":\"user prompt\"}');",
    )
    .unwrap();
}

#[test]
fn opencode_sqlite_scans_skip_oversized_rows_on_a_large_store() {
    if require_plugins().is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let db_dir = dir.path().join("opencode");
    std::fs::create_dir_all(&db_dir).unwrap();
    build_large_opencode_db(&db_dir.join("opencode.db"));
    let _env = EnvGuard::new(&[("XDG_DATA_HOME", dir.path().to_str().unwrap())]);
    let j = load_js("opencode");

    let mut usage = j.model_group_usage();
    usage.sort_by(|a, b| a.group.cmp(&b.group));
    let totals: Vec<(&str, u64, u64, u64)> = usage
        .iter()
        .map(|u| {
            (
                u.group.as_str(),
                u.messages,
                u.input_tokens,
                u.output_tokens,
            )
        })
        .collect();
    assert_eq!(
        totals,
        vec![("alpha", 2000, 4000, 2000), ("beta", 1000, 2000, 1000)],
        "one aggregated row per group, the oversized row never parsed"
    );

    assert!(
        j.detect_session_model("/work/proj", SES_ID).is_some(),
        "model scan skips the newest oversized row instead of failing"
    );

    let chat = j.read_structured_chat("/work/proj", SES_ID, None);
    let contents: Vec<&str> = chat.messages.iter().map(|m| m.content.as_str()).collect();
    assert_eq!(contents, vec!["assistant reply", "user prompt"]);
    assert_eq!(chat.messages[0].kind, "assistant");
}
