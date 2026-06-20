//! Codex provider parity — JS-only (no native `AgentProvider`), so each case
//! drives the real `provider-codex/plugin.js` through `JsProvider` against a
//! realistic fixture and asserts the result round-trips into the native shape.
//!
//! Covered capabilities: buildLaunchCommand, buildResumeCommand,
//! parseSessionDelta, parseSessionMessages, sessionFilePath, sessionJsonlPath,
//! enumerateSessions, detectSessionModel, detectSessionId, detectActiveSession.

use super::*;

const CODEX_UUID: &str = "11111111-1111-1111-1111-111111111111";
const CODEX_STEM: &str = "rollout-2026-05-01T10-00-00-11111111-1111-1111-1111-111111111111";

fn codex() -> JsProvider {
    load_js("codex")
}

#[test]
fn build_launch_command_shape() {
    if require_plugins().is_none() {
        return;
    }
    let j = codex();
    // Plain launch: codex binary + hook-trust bypass, no approval bypass.
    let plain = j.build_launch_command(&AgentLaunchParams::default());
    assert!(plain.starts_with("codex"), "must invoke codex: {plain}");
    assert!(
        plain.contains("--dangerously-bypass-hook-trust"),
        "hook-trust bypass always present: {plain}"
    );
    assert!(
        !plain.contains("--dangerously-bypass-approvals-and-sandbox"),
        "no approval bypass without skip: {plain}"
    );

    // skip_permissions toggles the approvals-and-sandbox bypass.
    let skip = j.build_launch_command(&AgentLaunchParams {
        skip_permissions: true,
        ..Default::default()
    });
    assert!(
        skip.contains("--dangerously-bypass-approvals-and-sandbox"),
        "skip must add approval bypass: {skip}"
    );

    // Caller args are shell-quoted onto the tail.
    let with_args = j.build_launch_command(&AgentLaunchParams {
        args: Some(vec!["a b".into()]),
        ..Default::default()
    });
    assert!(
        with_args.contains("'a b'") || with_args.contains("\"a b\""),
        "arg must be quoted: {with_args}"
    );
}

#[test]
fn build_resume_command_shape() {
    if require_plugins().is_none() {
        return;
    }
    let j = codex();
    // Resume accepts a rollout filename stem and extracts the trailing uuid.
    let cmd = j.build_resume_command(CODEX_STEM, true).unwrap();
    assert!(cmd.starts_with("codex resume"), "resume subcommand: {cmd}");
    assert!(cmd.contains(CODEX_UUID), "must carry the uuid: {cmd}");
    assert!(
        cmd.contains("--dangerously-bypass-approvals-and-sandbox"),
        "skip flag must propagate: {cmd}"
    );

    // Without skip, no approval bypass.
    let no_skip = j.build_resume_command(CODEX_UUID, false).unwrap();
    assert!(!no_skip.contains("--dangerously-bypass-approvals-and-sandbox"));
    assert!(no_skip.contains(CODEX_UUID));
}

#[test]
fn parse_session_delta_maps_response_items() {
    if require_plugins().is_none() {
        return;
    }
    let j = codex();
    let jsonl = concat!(
        r#"{"type":"session_meta","payload":{"id":"11111111-1111-1111-1111-111111111111","cwd":"/work/proj"}}"#,
        "\n",
        r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hello codex"}]},"timestamp":"2026-05-01T10:00:00Z"}"#,
        "\n",
        r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"hi there"}]}}"#,
        "\n",
        r#"{"type":"response_item","payload":{"type":"function_call","name":"shell","arguments":"{\"cmd\":\"ls -la\"}"}}"#,
        "\n",
        r#"{"type":"response_item","payload":{"type":"function_call_output","output":"file listing"}}"#,
        "\n",
        r#"{"type":"turn_context","payload":{"model":"gpt-5-codex"}}"#,
        "\n",
    );
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), jsonl).unwrap();

    let delta = j.parse_session_delta(tmp.path(), 0);
    assert_eq!(delta.new_offset, jsonl.len() as u64, "consumes whole file");
    // session_meta + turn_context dropped; message/function_call/_output kept.
    assert_eq!(delta.messages.len(), 4, "{:#?}", delta.messages);

    let m0 = &delta.messages[0];
    assert_eq!(m0.role, "user");
    assert_eq!(m0.blocks[0].kind, "text");
    assert_eq!(
        m0.blocks[0].data.get("text").and_then(|v| v.as_str()),
        Some("hello codex")
    );
    // Codex must forward the raw ISO timestamp (parity with claude): RichMessage.timestamp ->
    // ChatMsg.timestamp is what the optimistic-echo matcher keys on. Emitting only `ts` (seconds)
    // left codex echoes timeless, forcing content-only dedupe (the CE1 "codex null-ts" finding).
    assert_eq!(
        m0.timestamp.as_deref(),
        Some("2026-05-01T10:00:00Z"),
        "codex must forward ISO timestamp for echo matching, not null"
    );

    let m1 = &delta.messages[1];
    assert_eq!(m1.role, "assistant");
    assert_eq!(m1.blocks[0].kind, "text");
    assert_eq!(
        m1.timestamp, None,
        "a row with no timestamp stays null (honest)"
    );

    let m2 = &delta.messages[2];
    assert_eq!(m2.blocks[0].kind, "tool_use");
    assert_eq!(
        m2.blocks[0].data.get("name").and_then(|v| v.as_str()),
        Some("shell")
    );

    let m3 = &delta.messages[3];
    assert_eq!(m3.blocks[0].kind, "tool_result");
}

#[test]
fn parse_session_messages_keeps_only_text_messages() {
    if require_plugins().is_none() {
        return;
    }
    let j = codex();
    let jsonl = concat!(
        r#"{"type":"session_meta","payload":{"id":"x","cwd":"/p"}}"#,
        "\n",
        r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hello codex"}]},"timestamp":"2026-05-01T10:00:00Z"}"#,
        "\n",
        r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"hi there"}]}}"#,
        "\n",
        r#"{"type":"response_item","payload":{"type":"function_call","name":"shell","arguments":"{}"}}"#,
        "\n",
        "garbage-not-json\n",
    );
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), jsonl).unwrap();

    let msgs = j.parse_session_messages(tmp.path());
    // function_call + meta + malformed dropped; two text messages survive.
    assert_eq!(msgs.len(), 2, "{msgs:#?}");
    assert_eq!(msgs[0].role, "user");
    assert_eq!(msgs[0].text, "hello codex");
    assert_eq!(msgs[1].role, "assistant");
    assert_eq!(msgs[1].text, "hi there");
    assert!(!msgs[0].is_meta);
    // RFC3339 timestamp parsed to unix seconds and carried through.
    assert!(
        msgs[0].ts.is_some_and(|t| t > 0),
        "ts must parse: {:?}",
        msgs[0].ts
    );
}

#[test]
fn detect_session_id_and_active_session_are_null() {
    if require_plugins().is_none() {
        return;
    }
    let j = codex();
    // Codex binds sessions via the SessionStart hook (not file-scan detection),
    // so both detection hooks intentionally return None — assert the JS path
    // exercises and round-trips that null, not a guessed sibling session.
    assert_eq!(j.detect_session_id("/work/proj", &[], None), None);
    assert_eq!(j.detect_active_session("/work/proj", "current-sid"), None);
}

/// Filesystem-backed capabilities, driven through the `CODETERM_CODEX_SESSIONS_ROOT`
/// override so the plugin walks a controlled rollout tree instead of `~/.codex`.
/// One test owns the env var (serialized via `EnvGuard`) so the cases share a
/// single fixture and can't race a sibling reader.
#[test]
fn session_fs_capabilities_over_rollout_tree() {
    if require_plugins().is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("2026").join("05").join("01");
    std::fs::create_dir_all(&nested).unwrap();
    let file = nested.join(format!("{CODEX_STEM}.jsonl"));
    let contents = concat!(
        r#"{"type":"session_meta","payload":{"id":"11111111-1111-1111-1111-111111111111","cwd":"/work/proj"}}"#,
        "\n",
        r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"hi"}]}}"#,
        "\n",
        r#"{"type":"turn_context","payload":{"model":"gpt-5-codex"}}"#,
        "\n",
    );
    std::fs::write(&file, contents).unwrap();

    let _env = EnvGuard::new(&[("CODETERM_CODEX_SESSIONS_ROOT", dir.path().to_str().unwrap())]);
    let j = codex();

    // sessionFilePath / sessionJsonlPath resolve the rollout by its trailing uuid.
    let fp = j
        .session_file_path("/work/proj", CODEX_UUID)
        .expect("sessionFilePath must resolve the rollout");
    assert_eq!(fp, file, "resolved path must be the rollout file");
    let jp = j
        .session_jsonl_path("/work/proj", CODEX_UUID)
        .expect("sessionJsonlPath delegates to sessionFilePath");
    assert_eq!(jp, file);

    // enumerateSessions reads the session_meta header into native SessionMetadata.
    let sessions = j.enumerate_sessions();
    assert_eq!(sessions.len(), 1, "{sessions:#?}");
    let s = &sessions[0];
    assert_eq!(s.session_id, CODEX_UUID);
    assert_eq!(s.provider, "codex");
    assert_eq!(s.project_path.as_deref(), Some("/work/proj"));
    assert_eq!(s.file_path, file.to_string_lossy());

    // detectSessionModel reads turn_context.model → short label → catalog id.
    let model = j
        .detect_session_model("/work/proj", CODEX_UUID)
        .expect("detectSessionModel must resolve a model");
    assert!(!model.is_empty(), "resolved model id must be non-empty");
}
