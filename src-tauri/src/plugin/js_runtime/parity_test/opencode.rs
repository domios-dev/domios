//! OpenCode (sst/opencode) provider parity — JS-only (no native `AgentProvider`).
//! Each case drives the real `provider-opencode/plugin.js` through `JsProvider`.
//!
//! Covered: buildLaunchCommand, buildResumeCommand, usesStructuredChat,
//! readStructuredChat, sessionExists, sessionCreatedMs, detectSessionModel,
//! detectSessionId, detectActiveSession.
//!
//! OpenCode keeps everything in a sqlite db under `$XDG_DATA_HOME/opencode`.
//! The sqlite-backed cases build a controlled db in a tempdir and point the
//! plugin at it via `XDG_DATA_HOME` (serialized through `EnvGuard`).

use super::*;

const SES_ID: &str = "ses_abc";
const TC_SESSION: i64 = 1_700_000_000_000;
const TC_PART: i64 = 1_700_000_000_002;

fn opencode() -> JsProvider {
    load_js("opencode")
}

#[test]
fn build_launch_command_shape() {
    if require_plugins().is_none() {
        return;
    }
    let j = opencode();
    assert_eq!(
        j.build_launch_command(&AgentLaunchParams::default()),
        "opencode"
    );
    let with_args = j.build_launch_command(&AgentLaunchParams {
        args: Some(vec!["x y".into()]),
        ..Default::default()
    });
    assert!(with_args.starts_with("opencode"), "{with_args}");
    assert!(
        with_args.contains("'x y'") || with_args.contains("\"x y\""),
        "arg quoted: {with_args}"
    );
}

#[test]
fn build_resume_command_shape() {
    if require_plugins().is_none() {
        return;
    }
    let j = opencode();
    // Resume goes through the normal TUI path: `opencode --session <id>`.
    let cmd = j.build_resume_command(SES_ID, true).unwrap();
    assert!(cmd.contains("opencode"), "{cmd}");
    assert!(cmd.contains("--session"), "{cmd}");
    assert!(cmd.contains(SES_ID), "{cmd}");
}

#[test]
fn uses_structured_chat_is_true() {
    if require_plugins().is_none() {
        return;
    }
    // opencode chat lives in sqlite (part JOIN message), not a tailable JSONL.
    assert!(opencode().uses_structured_chat());
}

#[test]
fn detect_session_id_and_active_session_are_null() {
    if require_plugins().is_none() {
        return;
    }
    let j = opencode();
    // Binding happens via the installed opencode plugin (session_start hook),
    // so both file-scan detection hooks return None by design.
    assert_eq!(j.detect_session_id("/work/proj", &[], None), None);
    assert_eq!(j.detect_active_session("/work/proj", "current"), None);
}

fn build_opencode_db(path: &std::path::Path) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE session (id TEXT PRIMARY KEY, time_created INTEGER);
         CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT);
         CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT, time_created INTEGER, data TEXT);",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session (id, time_created) VALUES (?1, ?2)",
        rusqlite::params![SES_ID, TC_SESSION],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, data) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![
            "msg1",
            SES_ID,
            TC_SESSION + 1,
            r#"{"role":"assistant","modelID":"anthropic/claude-sonnet-4"}"#
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO part (id, message_id, session_id, time_created, data) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            "p1",
            "msg1",
            SES_ID,
            TC_PART,
            r#"{"type":"text","text":"hello from opencode"}"#
        ],
    )
    .unwrap();
    // Close the writer so the plugin's read-only open sees a complete db.
    drop(conn);
}

/// Sqlite-backed capabilities driven through `XDG_DATA_HOME` so the plugin
/// reads a controlled `opencode.db`. One test owns the env var (serialized via
/// `EnvGuard`) and shares a single db fixture.
#[test]
fn sqlite_backed_session_capabilities() {
    if require_plugins().is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let db_dir = dir.path().join("opencode");
    std::fs::create_dir_all(&db_dir).unwrap();
    build_opencode_db(&db_dir.join("opencode.db"));

    let _env = EnvGuard::new(&[("XDG_DATA_HOME", dir.path().to_str().unwrap())]);
    let j = opencode();

    // sessionExists: present id → true, missing id → false.
    assert!(j.session_exists("/work/proj", SES_ID));
    assert!(!j.session_exists("/work/proj", "ses_missing"));

    // sessionCreatedMs → SystemTime at the recorded birth instant.
    let born = j
        .session_created_at("/work/proj", SES_ID)
        .expect("sessionCreatedMs must resolve");
    let expected = std::time::UNIX_EPOCH + std::time::Duration::from_millis(TC_SESSION as u64);
    assert_eq!(born, expected);
    assert!(j.session_created_at("/work/proj", "ses_missing").is_none());

    // detectSessionModel reads this session's own assistant modelID → catalog id.
    let model = j
        .detect_session_model("/work/proj", SES_ID)
        .expect("detectSessionModel must resolve a model");
    assert!(!model.is_empty());

    // readStructuredChat joins part⋈message into native ChatMsg + advances cursor.
    let chat = j.read_structured_chat("/work/proj", SES_ID, None);
    assert_eq!(chat.messages.len(), 1, "{:#?}", chat.messages);
    let m = &chat.messages[0];
    assert_eq!(m.kind, "assistant", "role from message.data");
    assert_eq!(m.content, "hello from opencode");
    assert_eq!(
        chat.cursor.as_deref(),
        Some(TC_PART.to_string().as_str()),
        "cursor must be the max part.time_created seen"
    );

    // A cursor at/after the last part returns nothing new (incremental tail).
    let empty = j.read_structured_chat("/work/proj", SES_ID, Some(&(TC_PART + 1).to_string()));
    assert!(empty.messages.is_empty(), "{:#?}", empty.messages);
}
