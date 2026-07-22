//! Cursor provider parity — JS-only, driving the real `provider-cursor/plugin.js`
//! through `JsProvider`. The `~/.cursor`-backed reads have no env override (can't be
//! redirected without racing home-dependent cases), so they run against a non-existent
//! session to prove the honest negative round-trips into native types.

use super::*;

const CURSOR_UUID: &str = "abcdef01-2345-6789-abcd-ef0123456789";

fn cursor() -> JsProvider {
    load_js("cursor")
}

#[test]
fn build_launch_command_shape() {
    if require_plugins().is_none() {
        return;
    }
    let j = cursor();
    // Plain: bare cursor-agent, no force, no trust pre-write.
    let plain = j.build_launch_command(&AgentLaunchParams::default());
    // sec_hook_ampersand: every launch disables cursor-agent's import of the
    // user's ~/.claude/settings.json PreToolUse hooks (a Claude-dialect hook
    // body crashes Cursor's bash tool). Must lead, and never touch the file.
    let hook_disable_pos = plain
        .find("CLAUDE_CODE_DISABLE_HOOKS")
        .expect("hook-import disable prefix must be present");
    assert!(
        hook_disable_pos < plain.find("cursor-agent").unwrap(),
        "hook-import disable must lead the launch command: {plain}"
    );
    assert!(!plain.contains("--force"), "no force without skip: {plain}");
    assert!(
        !plain.contains(".workspace-trusted"),
        "no trust pre-write without skip: {plain}"
    );

    // skip_permissions → --force AND the workspace-trust marker pre-write prefix.
    let skip = j.build_launch_command(&AgentLaunchParams {
        skip_permissions: true,
        ..Default::default()
    });
    assert!(skip.contains("--force"), "skip adds --force: {skip}");
    assert!(
        skip.contains(".workspace-trusted"),
        "skip pre-writes cursor's trust marker: {skip}"
    );
    assert!(skip.contains("cursor-agent"), "{skip}");
}

#[test]
fn build_resume_command_shape() {
    if require_plugins().is_none() {
        return;
    }
    let j = cursor();
    let cmd = j.build_resume_command(CURSOR_UUID, true).unwrap();
    let hook_disable_pos = cmd
        .find("CLAUDE_CODE_DISABLE_HOOKS")
        .expect("hook-import disable prefix must be present");
    assert!(
        hook_disable_pos < cmd.find("cursor-agent").unwrap(),
        "hook-import disable must lead the resume command: {cmd}"
    );
    assert!(cmd.contains("--resume"), "{cmd}");
    assert!(cmd.contains(CURSOR_UUID), "{cmd}");
    assert!(cmd.contains("--force"), "skip adds --force: {cmd}");
    assert!(
        cmd.contains(".workspace-trusted"),
        "skip pre-writes trust: {cmd}"
    );

    let no_skip = j.build_resume_command(CURSOR_UUID, false).unwrap();
    assert!(!no_skip.contains("--force"));
    assert!(!no_skip.contains(".workspace-trusted"));
    assert!(no_skip.contains(CURSOR_UUID));
}

#[test]
fn parse_session_delta_strips_user_query_and_maps_tool_use() {
    if require_plugins().is_none() {
        return;
    }
    let j = cursor();
    let jsonl = concat!(
        r#"{"role":"user","message":{"content":[{"type":"text","text":"<timestamp>Monday, Jul 20, 2026, 1:21 PM (UTC+3)</timestamp>\n<user_query>\nrefactor this\n</user_query>"}]}}"#,
        "\n",
        r#"{"role":"assistant","message":{"content":[{"type":"text","text":"on it"},{"type":"tool_use","name":"Edit","input":{"path":"a.rs"}}]}}"#,
        "\n",
        r#"{"type":"turn_ended","foo":1}"#,
        "\n",
    );
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), jsonl).unwrap();

    let delta = j.parse_session_delta(tmp.path(), 0);
    assert_eq!(delta.new_offset, jsonl.len() as u64);
    // turn_ended dropped; user + assistant kept.
    assert_eq!(delta.messages.len(), 2, "{:#?}", delta.messages);

    let user = &delta.messages[0];
    assert_eq!(user.role, "user");
    assert_eq!(user.blocks[0].kind, "text");
    assert_eq!(
        user.blocks[0].data.get("text").and_then(|v| v.as_str()),
        Some("refactor this"),
        "timestamp and user_query framing must be stripped"
    );

    let asst = &delta.messages[1];
    assert_eq!(asst.role, "assistant");
    assert_eq!(asst.blocks.len(), 2);
    assert_eq!(asst.blocks[0].kind, "text");
    assert_eq!(asst.blocks[1].kind, "tool_use");
    assert_eq!(
        asst.blocks[1].data.get("name").and_then(|v| v.as_str()),
        Some("Edit")
    );
    assert_eq!(
        asst.blocks[1]
            .data
            .get("tool_status")
            .and_then(|v| v.as_str()),
        Some("success")
    );
}

#[test]
fn parse_session_messages_is_text_only() {
    if require_plugins().is_none() {
        return;
    }
    let j = cursor();
    let jsonl = concat!(
        r#"{"role":"user","message":{"content":[{"type":"text","text":"<user_query>\nhello\n</user_query>"}]}}"#,
        "\n",
        r#"{"role":"assistant","message":{"content":[{"type":"text","text":"hi"},{"type":"tool_use","name":"Edit","input":{}}]}}"#,
        "\n",
        r#"{"type":"turn_ended"}"#,
        "\n",
    );
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), jsonl).unwrap();

    let msgs = j.parse_session_messages(tmp.path());
    assert_eq!(msgs.len(), 2, "{msgs:#?}");
    assert_eq!(msgs[0].role, "user");
    assert_eq!(msgs[0].text, "hello");
    assert_eq!(msgs[1].role, "assistant");
    assert_eq!(msgs[1].text, "hi"); // tool_use carries no text
}

#[test]
fn uses_structured_chat_is_false() {
    if require_plugins().is_none() {
        return;
    }
    // Cursor tails an append-only JSONL transcript, NOT a structured store.
    assert!(!cursor().uses_structured_chat());
}

#[test]
fn session_jsonl_path_encodes_slug_and_session() {
    if require_plugins().is_none() {
        return;
    }
    let j = cursor();
    let p = j
        .session_jsonl_path("/work/myproj", CURSOR_UUID)
        .expect("should return the slug-derived transcript path");
    let s = p.to_string_lossy();
    assert!(s.contains("work-myproj"), "cwd slug missing: {s}");
    assert!(s.contains(CURSOR_UUID), "session id missing: {s}");
    assert!(
        s.contains("agent-transcripts"),
        "transcript dir missing: {s}"
    );
    assert!(
        s.contains(".cursor") && s.contains("projects"),
        "must live under ~/.cursor/projects: {s}"
    );
    // sessionFilePath resolves to the same transcript path.
    assert_eq!(j.session_file_path("/work/myproj", CURSOR_UUID), Some(p));
}

#[test]
fn session_jsonl_path_none_for_empty_sid() {
    if require_plugins().is_none() {
        return;
    }
    assert!(cursor().session_jsonl_path("/work/myproj", "").is_none());
}

#[test]
fn detection_and_existence_negatives_round_trip() {
    if require_plugins().is_none() {
        return;
    }
    let j = cursor();
    // A cwd with no chats dir → None (never a sibling guess); detectActiveSession is a stub → None.
    let bogus_cwd = "/nonexistent/cursor/parity/probe/xyz";
    assert_eq!(j.detect_session_id(bogus_cwd, &[], None), None);
    assert_eq!(j.detect_active_session(bogus_cwd, "current"), None);
    // No meta.json/store.db under a random session dir → exists=false, birth=None.
    assert!(!j.session_exists(bogus_cwd, CURSOR_UUID));
    assert!(j.session_created_at(bogus_cwd, CURSOR_UUID).is_none());
    // enumerateSessions yields a well-formed list, every entry tagged as a cursor session.
    assert!(j
        .enumerate_sessions()
        .iter()
        .all(|s| s.provider == "cursor"));
}

#[test]
fn cursor_stays_on_native_prompt_detector() {
    if require_plugins().is_none() {
        return;
    }
    assert!(!cursor().supports_structured_prompts());
}

#[test]
fn cursor_tui_usage_screen_is_skip_not_error() {
    if require_plugins().is_none() {
        return;
    }
    let provider = cursor();
    let parser = provider.usage_parser().expect("cursor provides parseUsage");
    let outcome = parser.parse_outcome("any tui screen text").unwrap();
    assert!(matches!(
        outcome,
        crate::usage_parser::UsageParseOutcome::Skip(_)
    ));
}
