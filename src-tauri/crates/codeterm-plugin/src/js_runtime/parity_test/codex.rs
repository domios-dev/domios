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
    let assigned_prompt = provider
        .starter_prompt_text(crate::provider::StarterPrompt::AssignedTask)
        .expect("Codex plugin must provide the assigned-task starter");
    let idle_prompt = provider
        .starter_prompt_text(crate::provider::StarterPrompt::IdleWakeup)
        .expect("Codex plugin must provide the idle starter");
    let team_prompt = provider
        .starter_prompt_text(crate::provider::StarterPrompt::TeamBootstrap)
        .expect("Codex plugin must provide the team starter");
    assert_ne!(assigned_prompt, idle_prompt);
    assert_ne!(assigned_prompt, team_prompt);
    assert_ne!(idle_prompt, team_prompt);
    assert!(assigned.contains(&assigned_prompt));
    assert!(idle.contains(&idle_prompt));
    assert!(!idle.contains(&assigned_prompt));
    assert!(team.contains(&team_prompt));
    assert!(!team.contains(&assigned_prompt));
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
        r#"{"type":"turn_context","payload":{"model":"gpt-5.6-sol"}}"#,
        "\n",
    );
    std::fs::write(&file, contents).unwrap();

    let _env = EnvGuard::new(&[("CODETERM_CODEX_SESSIONS_ROOT", dir.path().to_str().unwrap())]);
    let j = codex();

    assert_eq!(
        j.detect_session_id("/work/proj", &[], None).as_deref(),
        Some(CODEX_UUID)
    );
    assert_eq!(j.detect_session_id("/other/proj", &[], None), None);

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

    // detectSessionModel preserves the exact model reported by this session.
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
