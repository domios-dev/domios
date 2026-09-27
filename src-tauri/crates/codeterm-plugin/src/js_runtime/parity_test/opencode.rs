//! OpenCode provider parity — JS-only, driving the real `provider-opencode/plugin.js`
//! through `JsProvider`. OpenCode keeps sessions in SQLite under its XDG data
//! directory or `OPENCODE_DB`; fixture stores are isolated through `EnvGuard`.

use super::*;
use crate::provider::SystemPromptDelivery;

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
fn restored_resume_command_preserves_exact_session_model_and_effort() {
    if require_plugins().is_none() {
        return;
    }
    let command = opencode()
        .build_resume_command_with_context(&crate::provider::AgentResumeParams {
            session_id: SES_ID.into(),
            skip_permissions: false,
            system_prompt: None,
            args: vec![
                "-m".into(),
                "big-pickle".into(),
                "--variant".into(),
                "high".into(),
            ],
        })
        .expect("opencode restored resume command");

    assert_eq!(
        command,
        format!("opencode --session '{SES_ID}' -m 'big-pickle' --variant 'high'")
    );
}

/// OpenCode's `session_config` channel is a shell prologue that stages the
/// prompt file and exports `OPENCODE_CONFIG_CONTENT`. A resume drops it, so it
/// declares re-application and rebuilds the same channel around the resume.
#[test]
fn resume_with_context_restages_the_session_config() {
    if require_plugins().is_none() {
        return;
    }
    let j = opencode();
    let prompt = "## Composed identity\nopencode-sentinel-6b40";
    let command = j
        .build_resume_command_with_context(&crate::provider::AgentResumeParams {
            session_id: SES_ID.into(),
            skip_permissions: false,
            system_prompt: Some(prompt.into()),
            args: vec![
                "-m".into(),
                "big-pickle".into(),
                "--variant".into(),
                "high".into(),
            ],
        })
        .expect("opencode resume command");

    assert_eq!(command.matches("--session").count(), 1, "{command}");
    assert_eq!(
        command.matches("OPENCODE_CONFIG_CONTENT").count(),
        1,
        "{command}"
    );
    assert!(
        command.contains("opencode --session 'ses_abc' -m 'big-pickle' --variant 'high'"),
        "{command}"
    );
    assert!(
        !command.contains(prompt),
        "prompt travels in the staged file, not the command: {command}"
    );
    let staged = command
        .split_whitespace()
        .find(|token| token.contains(".ct-task"))
        .expect("staged prompt file")
        .trim_matches(['\'', '"']);
    let delivered = std::fs::read_to_string(staged).expect("staged prompt file");
    assert_eq!(delivered.matches(prompt).count(), 1);
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
fn supports_structured_prompts_from_manifest() {
    if require_plugins().is_none() {
        return;
    }
    let j = opencode();
    assert!(j.supports_structured_prompts());
    assert!(j.describe().supports_structured_prompts);
}

#[test]
fn task_instructions_use_isolated_session_config() {
    if require_plugins().is_none() {
        return;
    }
    assert_eq!(
        opencode().system_prompt_delivery(),
        SystemPromptDelivery::SessionConfig
    );
}

#[test]
fn detect_session_id_no_db_returns_null() {
    if require_plugins().is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    // Empty dir — no opencode.db — detection must return null.
    let _env = EnvGuard::new(&[("XDG_DATA_HOME", dir.path().to_str().unwrap())]);
    let j = opencode();
    assert_eq!(j.detect_session_id("/work/proj", &[], None), None);
    assert_eq!(j.detect_active_session("/work/proj", "current"), None);
}

#[test]
fn detect_session_id_finds_recent_session() {
    if require_plugins().is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let db_dir = dir.path().join("opencode");
    std::fs::create_dir_all(&db_dir).unwrap();
    let db_path = db_dir.join("opencode.db");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE session (id TEXT PRIMARY KEY, directory TEXT, time_created INTEGER);",
    )
    .unwrap();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    conn.execute(
        "INSERT INTO session (id, directory, time_created) VALUES (?1, ?2, ?3)",
        rusqlite::params!["ses_fresh", "/work/proj", now_ms],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session (id, directory, time_created) VALUES (?1, ?2, ?3)",
        rusqlite::params!["ses_foreign", "/other/proj", now_ms + 1],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session (id, directory, time_created) VALUES (?1, ?2, ?3)",
        rusqlite::params!["ses_old", "/work/proj", 1_700_000_000_000i64],
    )
    .unwrap();
    let mut excluded = Vec::new();
    for index in 0..25 {
        let foreign = format!("ses_crowd_{index:02}");
        conn.execute(
            "INSERT INTO session (id, directory, time_created) VALUES (?1, ?2, ?3)",
            rusqlite::params![foreign, "/crowd/proj", now_ms + 100 + index],
        )
        .unwrap();
        let same_cwd = format!("ses_excluded_{index:02}");
        conn.execute(
            "INSERT INTO session (id, directory, time_created) VALUES (?1, ?2, ?3)",
            rusqlite::params![same_cwd, "/work/proj", now_ms + 200 + index],
        )
        .unwrap();
        excluded.push(same_cwd);
    }
    drop(conn);

    let _env = EnvGuard::new(&[("XDG_DATA_HOME", dir.path().to_str().unwrap())]);
    let j = opencode();
    // Recent session within default 300s window → found.
    assert_eq!(
        j.detect_session_id("/work/proj", &excluded, None),
        Some("ses_fresh".to_string())
    );
    // Excluded → skip to next; old session outside window → null.
    assert_eq!(
        j.detect_session_id(
            "/work/proj",
            &excluded
                .iter()
                .cloned()
                .chain(std::iter::once("ses_fresh".to_string()))
                .collect::<Vec<_>>(),
            None,
        ),
        None
    );
    assert_eq!(
        j.detect_session_id("/other/proj", &[], None),
        Some("ses_foreign".into())
    );
    // detectActiveSession still null (not implemented).
    assert_eq!(j.detect_active_session("/work/proj", "current"), None);
}

fn build_opencode_db(path: &std::path::Path) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE session (id TEXT PRIMARY KEY, directory TEXT, time_created INTEGER);
         CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT);
         CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT);",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session (id, directory, time_created) VALUES (?1, ?2, ?3)",
        rusqlite::params![SES_ID, "/work/proj", TC_SESSION],
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
        "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            "p1",
            "msg1",
            SES_ID,
            TC_PART,
            TC_PART,
            r#"{"type":"text","text":"hello from opencode"}"#
        ],
    )
    .unwrap();
    // Close the writer so the plugin's read-only open sees a complete db.
    drop(conn);
}

/// Sqlite-backed capabilities driven through `XDG_DATA_HOME` against a controlled
/// `opencode.db`; one test owns the env var (serialized via `EnvGuard`).
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
    let cursor = chat.cursor.expect("structured cursor");
    assert_eq!(cursor, format!(r#"[{TC_PART},["p1"]]"#));

    let conn = rusqlite::Connection::open(db_dir.join("opencode.db")).unwrap();
    conn.execute(
        "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            "p0",
            "msg1",
            SES_ID,
            TC_PART,
            TC_PART,
            r#"{"type":"text","text":"same timestamp peer"}"#
        ],
    )
    .unwrap();
    drop(conn);

    let peer = j.read_structured_chat("/work/proj", SES_ID, Some(&cursor));
    assert_eq!(peer.messages.len(), 1, "{:#?}", peer.messages);
    assert_eq!(peer.messages[0].content, "same timestamp peer");
    let peer_cursor = peer.cursor.expect("peer cursor");
    assert_eq!(peer_cursor, format!(r#"[{TC_PART},["p0","p1"]]"#));
    let empty = j.read_structured_chat("/work/proj", SES_ID, Some(&peer_cursor));
    assert!(empty.messages.is_empty(), "{:#?}", empty.messages);

    // A streamed part keeps its id and creation instant while its content grows;
    // the cursor tracks the update instant, so the growth must be re-delivered.
    let grown_updated = TC_PART + 1_000;
    let conn = rusqlite::Connection::open(db_dir.join("opencode.db")).unwrap();
    conn.execute(
        "UPDATE part SET time_updated = ?1, data = ?2 WHERE id = ?3",
        rusqlite::params![
            grown_updated,
            r#"{"type":"text","text":"hello from opencode, grown"}"#,
            "p1"
        ],
    )
    .unwrap();
    drop(conn);

    let grown = j.read_structured_chat("/work/proj", SES_ID, Some(&peer_cursor));
    assert_eq!(grown.messages.len(), 1, "{:#?}", grown.messages);
    assert_eq!(grown.messages[0].content, "hello from opencode, grown");
    let grown_cursor = grown.cursor.expect("grown cursor");
    assert_eq!(grown_cursor, format!(r#"[{grown_updated},["p1"]]"#));
    let settled = j.read_structured_chat("/work/proj", SES_ID, Some(&grown_cursor));
    assert!(settled.messages.is_empty(), "{:#?}", settled.messages);

    for malformed in ["1000", "not-json", r#"[1000,"legacy-id"]"#] {
        let refused = j.read_structured_chat("/work/proj", SES_ID, Some(malformed));
        assert!(refused.messages.is_empty(), "{:#?}", refused.messages);
        assert_eq!(refused.cursor.as_deref(), Some(malformed));
    }
}
