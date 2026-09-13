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

fn codex_status_fixture() -> String {
    std::fs::read_to_string(
        workspace_plugins_root()
            .join("provider-codex")
            .join("tests")
            .join("fixtures")
            .join("codex-status-v0-142-4.txt"),
    )
    .expect("fixture file must exist")
}

fn codex() -> JsProvider {
    load_js("codex")
}

#[test]
fn busy_turn_steers_after_the_next_tool_call() {
    if require_plugins().is_none() {
        return;
    }
    assert_eq!(
        codex().busy_submit_strategy(),
        crate::provider::BusySubmitStrategy::SubmitNormally
    );
}

#[test]
fn build_launch_command_shape() {
    if require_plugins().is_none() {
        return;
    }
    let j = codex();
    // Plain launch: codex binary only, no bypass flags.
    let plain = j.build_launch_command(&AgentLaunchParams::default());
    assert!(plain.starts_with("codex"), "must invoke codex: {plain}");
    assert!(
        !plain.contains("--dangerously-bypass-hook-trust"),
        "removed Codex CLI flag must stay absent: {plain}"
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
fn idle_starter_case() {
    if require_plugins().is_none() {
        return;
    }
    let provider = codex();
    let task = "manager identity arrives in the developer channel";
    let assigned = provider.build_launch_command(&AgentLaunchParams {
        task: Some(task.into()),
        ..Default::default()
    });
    let idle = provider.build_launch_command(&AgentLaunchParams {
        task: Some(task.into()),
        starter_prompt: crate::provider::StarterPrompt::IdleWakeup,
        ..Default::default()
    });
    let team = provider.build_launch_command(&AgentLaunchParams {
        task: Some(task.into()),
        starter_prompt: crate::provider::StarterPrompt::TeamBootstrap,
        ..Default::default()
    });
    let startup = provider.build_launch_command(&AgentLaunchParams {
        task: Some(task.into()),
        starter_prompt: crate::provider::StarterPrompt::StartupHandshake,
        ..Default::default()
    });
    let assigned_prompt = provider
        .starter_prompt_text(crate::provider::StarterPrompt::AssignedTask)
        .expect("Codex plugin must provide the assigned-task starter");
    let idle_prompt = provider
        .starter_prompt_text(crate::provider::StarterPrompt::IdleWakeup)
        .expect("Codex plugin must provide the idle starter");
    let team_prompt = provider
        .starter_prompt_text(crate::provider::StarterPrompt::TeamBootstrap)
        .expect("Codex plugin must provide the team starter");
    let startup_prompt = provider
        .starter_prompt_text(crate::provider::StarterPrompt::StartupHandshake)
        .expect("host startup handshake must reach the provider");
    assert_ne!(assigned_prompt, idle_prompt);
    assert_ne!(assigned_prompt, team_prompt);
    assert_ne!(idle_prompt, team_prompt);
    assert!(assigned.contains(&assigned_prompt));
    assert!(idle.contains(&idle_prompt));
    assert!(!idle.contains(&assigned_prompt));
    assert!(team.contains(&team_prompt));
    assert!(!team.contains(&assigned_prompt));
    assert!(startup.contains(&startup_prompt));
    assert!(!startup.contains(&assigned_prompt));
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
    assert!(!no_skip.contains("--dangerously-bypass-hook-trust"));
    assert!(no_skip.contains(CODEX_UUID));
}

/// Codex declares no resume re-application, so the composed prompt must not
/// reach its command — the conservative default that keeps a retaining
/// provider from carrying two copies of its primer. Launch args still travel.
#[test]
fn resume_with_context_keeps_the_conservative_default() {
    if require_plugins().is_none() {
        return;
    }
    let j = codex();
    let params = crate::provider::AgentResumeParams {
        session_id: CODEX_UUID.into(),
        skip_permissions: false,
        system_prompt: Some("## Composed identity\nparity-sentinel-91c3".into()),
        args: vec!["--model".into(), "parity-model".into()],
    };
    let command = j
        .build_resume_command_with_context(&params)
        .expect("codex resume command");

    assert!(!command.contains("parity-sentinel-91c3"));
    assert_eq!(
        command,
        crate::provider::append_launch_args(
            j.build_resume_command(CODEX_UUID, false)
                .expect("codex base resume"),
            &params.args
        )
    );
}

#[test]
fn directory_trust_prompt_response_is_provider_declared() {
    if require_plugins().is_none() {
        return;
    }
    use codeterm_types::pty::{PendingPrompt, PromptOption};
    let prompt = PendingPrompt {
        question: "Do you trust the contents of this directory?".into(),
        options: vec![
            PromptOption {
                label: "Yes, continue".into(),
                value: "1".into(),
            },
            PromptOption {
                label: "No, quit".into(),
                value: "2".into(),
            },
        ],
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    };
    assert!(codex()
        .directory_trust_prompt_response(&prompt, "1. Yes, continue")
        .is_some());
    let foreign_prompt = PendingPrompt {
        question: "Approve this command?".into(),
        ..prompt
    };
    assert!(codex()
        .directory_trust_prompt_response(&foreign_prompt, "1. Yes, continue")
        .is_none());
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
    // ChatMsg.timestamp is what the optimistic-echo matcher keys on. Emitting only `ts` (seconds)
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
fn parse_usage_uses_family_labels_for_codex_142_4_status() {
    if require_plugins().is_none() {
        return;
    }
    let provider = codex();
    let parser = provider.usage_parser().expect("codex provides parseUsage");
    let status = codex_status_fixture();
    let usage = parser.parse(&status).expect("status should parse");
    assert_eq!(usage.provider, "codex");
    assert_eq!(usage.session_pct, Some(82.0));
    assert_eq!(usage.weekly_pct, Some(56.0));
    assert!(
        usage.session_resets_at_ms.is_some(),
        "session reset should parse"
    );
    assert!(
        usage.weekly_resets_at_ms.is_some(),
        "weekly reset should parse"
    );
}

#[test]
fn detect_active_session_is_null() {
    if require_plugins().is_none() {
        return;
    }
    let j = codex();
    assert_eq!(j.detect_active_session("/work/proj", "current-sid"), None);
}

/// Codex's current index and thread-history stores are the only session sources.
/// One test owns the env vars (serialized via `EnvGuard`) so the fixture cannot
/// race a sibling reader.
#[test]
fn session_fs_capabilities_over_current_stores() {
    if require_plugins().is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let index = dir.path().join("session_index.jsonl");
    let history = dir.path().join("thread_history_1.sqlite");
    std::fs::write(
        &index,
        format!(
            "{}\n{}\n{}\n",
            serde_json::json!({
                "id": CODEX_UUID,
                "thread_name": "project prompt",
                "updated_at": "2026-09-01T10:00:00Z"
            }),
            serde_json::json!({
                "id": "22222222-2222-2222-2222-222222222222",
                "thread_name": "foreign newer prompt",
                "updated_at": "2026-09-01T11:00:00Z"
            }),
            serde_json::json!({
                "id": CODEX_UUID,
                "thread_name": "older duplicate",
                "updated_at": "2026-09-01T09:00:00Z"
            }),
        ),
    )
    .unwrap();
    let db = rusqlite::Connection::open(&history).unwrap();
    db.execute_batch(
        "CREATE TABLE thread_items (
            thread_id TEXT, item_type TEXT, item_json TEXT,
            rollout_ordinal INTEGER, created_at_ms INTEGER
        );",
    )
    .unwrap();
    db.execute(
        "INSERT INTO thread_items VALUES (?1, 'thread_meta', ?2, 0, 0)",
        rusqlite::params![
            CODEX_UUID,
            r#"{"type":"thread_meta","payload":{"cwd":"/work/proj"}}"#
        ],
    )
    .unwrap();
    db.execute(
        "INSERT INTO thread_items VALUES (?1, 'turn_context', ?2, 1, 1)",
        rusqlite::params![
            CODEX_UUID,
            r#"{"type":"turn_context","payload":{"model":"gpt-5.6-sol"}}"#
        ],
    )
    .unwrap();
    db.execute(
        "INSERT INTO thread_items VALUES (?1, 'thread_meta', ?2, 0, 0)",
        rusqlite::params![
            "22222222-2222-2222-2222-222222222222",
            r#"{"type":"thread_meta","payload":{"cwd":"/other/proj"}}"#
        ],
    )
    .unwrap();

    let _env = EnvGuard::new(&[
        ("CODETERM_CODEX_SESSION_INDEX_PATH", index.to_str().unwrap()),
        (
            "CODETERM_CODEX_THREAD_HISTORY_PATH",
            history.to_str().unwrap(),
        ),
    ]);
    let j = codex();

    assert_eq!(
        j.detect_session_id(
            "/work/proj",
            &[],
            Some(std::time::Duration::from_secs(365 * 24 * 60 * 60)),
        )
        .as_deref(),
        Some(CODEX_UUID)
    );
    assert_eq!(j.detect_session_id("/other/proj", &[], None), None);

    assert_eq!(j.session_file_path("/work/proj", CODEX_UUID), None);
    assert_eq!(j.session_jsonl_path("/work/proj", CODEX_UUID), None);

    let sessions = j.enumerate_sessions();
    assert_eq!(sessions.len(), 2, "{sessions:#?}");
    let s = sessions
        .iter()
        .find(|session| session.session_id == CODEX_UUID)
        .expect("folded project session must be enumerated");
    assert_eq!(s.session_id, CODEX_UUID);
    assert_eq!(s.provider, "codex");
    assert_eq!(s.project_path.as_deref(), Some("/work/proj"));
    assert_eq!(s.file_path, index.to_string_lossy());

    let model = j
        .detect_session_model("/work/proj", CODEX_UUID)
        .expect("detectSessionModel must resolve a model");
    assert_eq!(model, "gpt-5.6-sol");
}

#[test]
fn codex_stays_on_native_prompt_detector() {
    if require_plugins().is_none() {
        return;
    }
    assert!(!codex().supports_structured_prompts());
}

#[test]
fn parse_session_delta_normalizes_reasoning_and_agent_activity() {
    if require_plugins().is_none() {
        return;
    }
    let jsonl = concat!(
        r#"{"type":"event_msg","timestamp":"2026-05-01T10:00:00Z","payload":{"type":"item_completed","turn_id":"turn-r","item":{"type":"Reasoning","id":"rs-a","summary_text":["**Inspect sources**","",7],"raw_content":["opaque-raw"]}}}"#,
        "\n",
        r#"{"type":"response_item","payload":{"type":"reasoning","id":"rs-a","summary":[{"type":"summary_text","text":"**Inspect sources**"}],"encrypted_content":"opaque-encrypted"}}"#,
        "\n",
        r#"{"type":"response_item","payload":{"type":"reasoning","id":"rs-empty","summary":[],"encrypted_content":"opaque-encrypted"}}"#,
        "\n",
        r#"{"type":"event_msg","payload":{"type":"item_completed","turn_id":"turn-r","item":{"type":"SubAgentActivity","id":"call-a","kind":"started","agent_thread_id":"thread-w","agent_path":"/root/worker"}}}"#,
        "\n",
        r#"{"type":"event_msg","payload":{"type":"item_completed","turn_id":"turn-r","item":{"type":"CollabAgentToolCall","id":"call-wait","tool":"wait","status":"completed","sender_thread_id":"thread-root","receiver_thread_ids":[],"receiver_agents":[],"agents_states":{}}}}"#,
        "\n",
        r#"{"type":"response_item","payload":{"type":"agent_message","id":"amsg-1","author":"/root","recipient":"/root/worker","content":[{"type":"encrypted_content","encrypted_content":"opaque-message"}],"internal_chat_message_metadata_passthrough":{"turn_id":"turn-r"}}}"#,
        "\n",
        r#"{"type":"response_item","payload":{"type":"agent_message","id":"amsg-2","author":"/root","recipient":"/root"}}"#,
        "\n",
    );
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), jsonl).unwrap();

    let delta = codex().parse_session_delta(tmp.path(), 0);
    let blocks: Vec<_> = delta.messages.iter().flat_map(|m| m.blocks.iter()).collect();
    let kinds: Vec<&str> = blocks.iter().map(|b| b.kind.as_str()).collect();
    assert_eq!(kinds, vec!["thinking", "tool_use", "tool_use"], "{:#?}", delta.messages);
    assert_eq!(blocks[0].data, serde_json::json!({ "text": "**Inspect sources**" }));
    assert_eq!(blocks[1].data["tool_kind"], "agent");
    assert_eq!(
        blocks[1].data["input"],
        serde_json::json!({ "agent_id": "thread-w", "agent_path": "/root/worker", "agent_state": "started" })
    );
    assert_eq!(blocks[1].data["tool_flow_id"], "turn-r");
    assert_eq!(blocks[2].data["tool_kind"], "agent");
    assert_eq!(
        blocks[2].data["input"],
        serde_json::json!({ "message_from": "/root", "message_to": "/root/worker" })
    );
    assert!(
        !format!("{:?}", delta.messages).contains("opaque"),
        "encrypted, raw, and message content never crosses"
    );
}
