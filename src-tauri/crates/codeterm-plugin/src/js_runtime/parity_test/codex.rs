//! Codex provider parity — JS-only (no native `AgentProvider`), so each case
//! drives the real `provider-codex/plugin.js` through `JsProvider` against a
//! realistic fixture and asserts the result round-trips into the native shape.
//!
//! Covered capabilities: buildLaunchCommand, buildResumeCommand,
//! buildResumeCommandWithContext,
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

fn codex_update_fixture() -> String {
    std::fs::read_to_string(
        workspace_plugins_root()
            .join("provider-codex")
            .join("tests")
            .join("fixtures")
            .join("directory-trust")
            .join("codex-update-available.txt"),
    )
    .expect("Codex update fixture")
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
fn codex_sandbox_writable_roots_native_js_parity() {
    if require_plugins().is_none() {
        return;
    }
    let provider = codex();
    assert_eq!(
        provider.additional_writable_dir_flag().as_deref(),
        Some("--add-dir")
    );
    let args = vec![
        "--add-dir".to_string(),
        "D:/repo/.git/worktrees/lane".to_string(),
        "--add-dir".to_string(),
        "D:/repo/.git/objects".to_string(),
        "--add-dir".to_string(),
        "D:/repo/.git/refs".to_string(),
        "--add-dir".to_string(),
        "D:/repo/.git/logs".to_string(),
    ];
    let launch = provider.build_launch_command(&AgentLaunchParams {
        args: Some(args.clone()),
        ..Default::default()
    });
    assert_eq!(launch.matches("--add-dir").count(), 4);
    let resume = provider
        .build_resume_command_with_context(&crate::provider::AgentResumeParams {
            session_id: CODEX_UUID.to_string(),
            args,
            instance_id: Some("parity-instance".to_string()),
            ..Default::default()
        })
        .expect("context resume command");
    assert_eq!(resume.matches("--add-dir").count(), 4);
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
    assert_eq!(
        plain
            .matches("-c check_for_update_on_startup=false")
            .count(),
        1
    );
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
    assert_eq!(
        skip.matches("-c check_for_update_on_startup=false").count(),
        1
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
    assert_eq!(
        with_args
            .matches("-c check_for_update_on_startup=false")
            .count(),
        1
    );
}

#[test]
fn classified_prompt_auto_response_is_limited_to_update_skip() {
    if require_plugins().is_none() {
        return;
    }
    let provider = codex();
    let screen = codex_update_fixture();
    assert!(provider.supports_classified_prompt_auto_response());
    assert_eq!(
        provider
            .classified_prompt_auto_response(crate::tab_state::TabState::UpdatePrompt, &screen,),
        Some(crate::provider::ClassifiedPromptOptionRole::Skip)
    );
    assert_eq!(
        provider.classified_prompt_auto_response(
            crate::tab_state::TabState::ClarifyingQuestion,
            &screen
        ),
        None
    );
    assert_eq!(
        provider.classified_prompt_auto_response(
            crate::tab_state::TabState::UpdatePrompt,
            &screen.replace("2. Skip", "2. Skip this version"),
        ),
        None
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
        instance_id: Some("instance-parity".into()),
        ..Default::default()
    });
    let idle = provider.build_launch_command(&AgentLaunchParams {
        task: Some(task.into()),
        starter_prompt: crate::provider::StarterPrompt::IdleWakeup,
        instance_id: Some("instance-parity".into()),
        ..Default::default()
    });
    let team = provider.build_launch_command(&AgentLaunchParams {
        task: Some(task.into()),
        starter_prompt: crate::provider::StarterPrompt::TeamBootstrap,
        instance_id: Some("instance-parity".into()),
        ..Default::default()
    });
    let startup = provider.build_launch_command(&AgentLaunchParams {
        task: Some(task.into()),
        starter_prompt: crate::provider::StarterPrompt::StartupHandshake,
        instance_id: Some("instance-parity".into()),
        ..Default::default()
    });
    for command in [&assigned, &idle, &team, &startup] {
        assert_eq!(
            command
                .matches("-c check_for_update_on_startup=false")
                .count(),
            1
        );
    }
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
    // Animations stay on: the repainting status band is what the offline state
    // classifier reads as a live turn, so silencing it would blind activity.
    assert!(cmd.contains(" resume"), "resume subcommand shape: {cmd}");
    assert_eq!(
        cmd.matches("-c check_for_update_on_startup=false").count(),
        1
    );
    assert!(cmd.contains(CODEX_UUID), "must carry the uuid: {cmd}");
    assert!(
        cmd.contains("--dangerously-bypass-approvals-and-sandbox"),
        "skip flag must propagate: {cmd}"
    );
    // Without skip, no approval bypass.
    let no_skip = j.build_resume_command(CODEX_UUID, false).unwrap();
    assert_eq!(
        no_skip
            .matches("-c check_for_update_on_startup=false")
            .count(),
        1
    );
    assert!(!no_skip.contains("--dangerously-bypass-approvals-and-sandbox"));
    assert!(!no_skip.contains("--dangerously-bypass-hook-trust"));
    assert!(no_skip.contains(CODEX_UUID));
}

#[test]
fn resume_with_context_restores_profile_and_launch_parameters() {
    if require_plugins().is_none() {
        return;
    }
    let j = codex();
    let params = crate::provider::AgentResumeParams {
        session_id: CODEX_UUID.into(),
        skip_permissions: true,
        agent_name: Some("restored-agent".into()),
        system_prompt: Some("## Composed identity\nparity-sentinel-91c3".into()),
        instance_id: Some("instance-parity".into()),
        args: vec![
            "-m".into(),
            "gpt-6-sol".into(),
            "-c".into(),
            r#"model_reasoning_effort="xhigh""#.into(),
        ],
    };
    let command = j
        .build_resume_command_with_context(&params)
        .expect("codex resume command");

    assert!(command.contains(" resume"));
    assert!(command.contains(CODEX_UUID));
    assert!(command.contains("--dangerously-bypass-approvals-and-sandbox"));
    assert!(command.contains("--dangerously-bypass-hook-trust"));
    assert!(command.contains("gpt-6-sol"));
    assert_eq!(
        command
            .matches("-c check_for_update_on_startup=false")
            .count(),
        1
    );
    assert!(command.contains("model_reasoning_effort"));
    assert!(command.contains("xhigh"));
    assert!(!command.contains("parity-sentinel-91c3"));
    if cfg!(windows) {
        assert!(command.contains("--profile $ctProfileName"));
        assert!(command.contains("launch-artifacts/release"));
        assert!(command.contains("instance-parity"));
    }
}

#[test]
fn resume_with_context_keeps_the_conservative_default() {
    if require_plugins().is_none() {
        return;
    }
    let provider = codex();
    let params = crate::provider::AgentResumeParams {
        session_id: CODEX_UUID.into(),
        skip_permissions: false,
        instance_id: Some("instance-parity".into()),
        system_prompt: Some("## Composed identity\nparity-sentinel-91c3".into()),
        args: vec![
            "-m".into(),
            "gpt-6-sol".into(),
            "-c".into(),
            r#"model_reasoning_effort="high""#.into(),
        ],
        ..Default::default()
    };
    let command = provider
        .build_resume_command_with_context(&params)
        .expect("codex resume command");

    assert!(!command.contains("parity-sentinel-91c3"));
    assert!(command.contains("--no-daemon"));
    assert!(command.contains("gpt-6-sol"));
    assert!(command.contains("model_reasoning_effort=\"high\""));
    if cfg!(windows) {
        assert!(command.contains("--profile $ctProfileName"));
    } else {
        assert_eq!(
            command,
            format!(
                "codex -c check_for_update_on_startup=false resume '{CODEX_UUID}' --no-daemon -m gpt-6-sol -c model_reasoning_effort=\"high\""
            )
        );
        assert_eq!(
            command,
            crate::provider::append_launch_args(
                provider
                    .build_resume_command(CODEX_UUID, false)
                    .expect("codex base resume"),
                &params.args
            )
        );
    }
}

#[test]
fn directory_trust_prompt_response_reads_wrapped_codex_trust_frames() {
    if require_plugins().is_none() {
        return;
    }
    use crate::provider::DirectoryTrustPromptResponse;
    let dir = workspace_plugins_root()
        .join("provider-codex")
        .join("tests")
        .join("fixtures")
        .join("directory-trust");
    let cases: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("cases.json")).expect("cases.json"))
            .expect("cases.json parses");
    let cases = cases.as_array().expect("case list");
    assert!(cases.len() >= 5, "every declared fixture is exercised");
    for case in cases {
        let file = case["file"].as_str().expect("file");
        let screen = std::fs::read_to_string(dir.join(file)).expect("fixture");
        let answer = |step: &[u8]| {
            Some(DirectoryTrustPromptResponse::Answer {
                step: step.to_vec(),
            })
        };
        let want = match case["expect"].as_str().expect("expect") {
            "enter" => answer(b"\r"),
            "move-up" => answer(b"\x1b[A"),
            "move-down" => answer(b"\x1b[B"),
            "hold" => Some(DirectoryTrustPromptResponse::Hold),
            "none" => None,
            other => panic!("unknown directory-trust expectation {other}"),
        };
        assert_eq!(
            codex().directory_trust_prompt_response(&screen),
            want,
            "{file}"
        );
    }
}

#[test]
fn codex_update_dialog_is_never_a_trust_dialog_native_js_parity() {
    if require_plugins().is_none() {
        return;
    }
    let fixture = workspace_plugins_root()
        .join("provider-codex")
        .join("tests")
        .join("fixtures")
        .join("directory-trust")
        .join("codex-update-available.txt");
    let screen = std::fs::read_to_string(fixture).expect("Codex update fixture");
    let provider = codex();
    assert_eq!(provider.directory_trust_prompt_response(&screen), None);
    let snapshot = crate::tab_state::TabStateSnapshot {
        tab_id: "update-tab".into(),
        agent_type: "codex".into(),
        session_id: None,
        cwd: None,
        screen_text: screen.clone(),
    };
    let classification = provider
        .classify_tab_state(&snapshot)
        .expect("update state");
    assert_eq!(
        classification.state,
        crate::tab_state::TabState::UpdatePrompt
    );
    assert!(classification.action.is_none());
    let prompt = provider.parse_prompt(&screen).expect("structured prompt");
    assert_eq!(
        prompt
            .options
            .iter()
            .map(|option| option.value.as_str())
            .collect::<Vec<_>>(),
        ["1", "2", "3"]
    );
    assert_eq!(
        provider.build_prompt_response(1, &screen),
        vec![b"\x1b[B".to_vec(), b"\r".to_vec()]
    );
    assert_eq!(
        provider.permission_prompt_response("Bash", &prompt, &screen),
        None
    );
}

#[test]
fn codex_update_copy_in_transcript_is_not_a_blocking_modal() {
    if require_plugins().is_none() {
        return;
    }
    let screen = std::fs::read_to_string(
        workspace_plugins_root()
            .join("provider-codex")
            .join("tests")
            .join("fixtures")
            .join("directory-trust")
            .join("codex-update-in-transcript.txt"),
    )
    .expect("historical-update fixture");
    let provider = codex();
    let snapshot = crate::tab_state::TabStateSnapshot {
        tab_id: "history-tab".into(),
        agent_type: "codex".into(),
        session_id: None,
        cwd: None,
        screen_text: screen,
    };

    let classification = provider
        .classify_tab_state(&snapshot)
        .expect("partial historical update evidence");
    assert_eq!(classification.state, crate::tab_state::TabState::Unknown);
    assert!(classification.confidence < 0.8);
}

#[test]
fn codex_folder_trust_prompt_parses_as_numbered_choices() {
    if require_plugins().is_none() {
        return;
    }
    let screen = std::fs::read_to_string(
        workspace_plugins_root()
            .join("provider-codex")
            .join("tests")
            .join("fixtures")
            .join("directory-trust")
            .join("trust-structured-question.txt"),
    )
    .expect("folder-trust fixture");
    let prompt = codex()
        .parse_directory_trust_prompt(&screen)
        .expect("active folder trust prompt");

    assert!(prompt.question.ends_with('?'));
    assert_eq!(
        prompt
            .options
            .iter()
            .map(|option| option.value.as_str())
            .collect::<Vec<_>>(),
        vec!["1", "2"]
    );
    assert_eq!(
        codex().build_prompt_response(0, &screen),
        vec![b"\x1b[A".to_vec(), b"\r".to_vec()]
    );

    let answer = std::fs::read_to_string(
        workspace_plugins_root()
            .join("provider-codex")
            .join("tests")
            .join("fixtures")
            .join("directory-trust")
            .join("approval-paragraph.txt"),
    )
    .expect("ordinary approval fixture");
    assert!(codex().parse_prompt(&answer).is_some());
    assert!(codex().parse_directory_trust_prompt(&answer).is_none());

    let fixtures = workspace_plugins_root()
        .join("provider-codex")
        .join("tests")
        .join("fixtures")
        .join("directory-trust");
    let cases: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(fixtures.join("cases.json")).expect("fixture cases"),
    )
    .expect("valid fixture cases");
    for case in cases.as_array().expect("fixture case list") {
        let file = case["file"].as_str().expect("fixture name");
        let expected = case["expect"].as_str().expect("fixture expectation") != "none";
        let screen = std::fs::read_to_string(fixtures.join(file)).expect("fixture frame");
        assert_eq!(
            codex().parse_directory_trust_prompt(&screen).is_some(),
            expected,
            "{file}"
        );
    }
}

#[test]
fn codex_trust_prompt_is_respondable_native_js_parity() {
    if require_plugins().is_none() {
        return;
    }
    let screen = std::fs::read_to_string(
        workspace_plugins_root()
            .join("provider-codex")
            .join("tests")
            .join("fixtures")
            .join("directory-trust")
            .join("trust-folder-access-0.157.1.txt"),
    )
    .expect("Codex 0.157.1 trust fixture");
    let provider = codex();
    let prompt = provider
        .parse_directory_trust_prompt(&screen)
        .expect("structured trust options are available to the host");
    assert_eq!(
        prompt
            .options
            .iter()
            .map(|option| option.value.as_str())
            .collect::<Vec<_>>(),
        ["1", "2"]
    );
    assert_eq!(
        provider.build_prompt_response(1, &screen),
        vec![b"\x1b[B".to_vec(), b"\r".to_vec()]
    );
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
    let state_db = dir.path().join("state_1.sqlite");
    // detectSessionId requires the project directory to exist on disk.
    let work_proj = dir.path().join("work-proj");
    let other_proj = dir.path().join("other-proj");
    std::fs::create_dir_all(&work_proj).unwrap();
    std::fs::create_dir_all(&other_proj).unwrap();
    let work_proj = work_proj.to_str().unwrap().to_string();
    let other_proj = other_proj.to_str().unwrap().to_string();
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
    // The plugin reads session cwd and model from the Codex state database's threads table.
    let db = rusqlite::Connection::open(&state_db).unwrap();
    db.execute_batch(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, model TEXT, rollout_path TEXT, cwd TEXT);",
    )
    .unwrap();
    db.execute(
        "INSERT INTO threads (id, model, rollout_path, cwd) VALUES (?1, 'gpt-5.6-sol', NULL, ?2)",
        rusqlite::params![CODEX_UUID, work_proj],
    )
    .unwrap();
    db.execute(
        "INSERT INTO threads (id, model, rollout_path, cwd) VALUES (?1, NULL, NULL, ?2)",
        rusqlite::params!["22222222-2222-2222-2222-222222222222", other_proj],
    )
    .unwrap();

    let _env = EnvGuard::new(&[
        ("CODETERM_CODEX_SESSION_INDEX_PATH", index.to_str().unwrap()),
        ("CODETERM_CODEX_STATE_DB_PATH", state_db.to_str().unwrap()),
    ]);
    let j = codex();

    assert_eq!(
        j.detect_session_id(
            &work_proj,
            &[],
            Some(std::time::Duration::from_secs(365 * 24 * 60 * 60)),
        )
        .as_deref(),
        Some(CODEX_UUID)
    );
    assert_eq!(j.detect_session_id(&other_proj, &[], None), None);

    assert_eq!(j.session_file_path(&work_proj, CODEX_UUID), None);
    assert_eq!(j.session_jsonl_path(&work_proj, CODEX_UUID), None);

    let sessions = j.enumerate_sessions();
    assert_eq!(sessions.len(), 2, "{sessions:#?}");
    let s = sessions
        .iter()
        .find(|session| session.session_id == CODEX_UUID)
        .expect("folded project session must be enumerated");
    assert_eq!(s.session_id, CODEX_UUID);
    assert_eq!(s.provider, "codex");
    assert_eq!(s.project_path.as_deref(), Some(work_proj.as_str()));
    assert_eq!(s.file_path, index.to_string_lossy());

    let model = j
        .detect_session_model(&work_proj, CODEX_UUID)
        .expect("detectSessionModel must resolve a model");
    assert_eq!(model, "gpt-5.6-sol");
}

#[test]
fn codex_session_reasoning_effort_reads_exact_rollout() {
    if require_plugins().is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let state_db = dir.path().join("state_1.sqlite");
    let rollout = dir.path().join("rollout-owned.jsonl");
    std::fs::write(
        &rollout,
        concat!(
            "{\"type\":\"turn_context\",\"payload\":{\"effort\":\"high\"}}\n",
            "{\"type\":\"turn_context\",\"payload\":{\"effort\":\"xhigh\"}}\n",
        ),
    )
    .unwrap();
    let db = rusqlite::Connection::open(&state_db).unwrap();
    db.execute_batch(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, model TEXT, rollout_path TEXT, cwd TEXT);",
    )
    .unwrap();
    db.execute(
        "INSERT INTO threads (id, model, rollout_path, cwd) VALUES (?1, 'gpt-5.6-sol', ?2, ?3)",
        rusqlite::params![
            CODEX_UUID,
            rollout.to_string_lossy().to_string(),
            dir.path().to_string_lossy().to_string()
        ],
    )
    .unwrap();

    let _env = EnvGuard::new(&[("CODETERM_CODEX_STATE_DB_PATH", state_db.to_str().unwrap())]);
    let provider = codex();
    assert_eq!(
        provider
            .detect_session_reasoning_effort("/ignored", CODEX_UUID)
            .as_deref(),
        Some("xhigh")
    );
    assert_eq!(
        provider
            .detect_session_reasoning_effort("/ignored", "22222222-2222-2222-2222-222222222222")
            .as_deref(),
        None,
        "a neighboring thread never supplies this session's actual effort"
    );
}

#[test]
fn codex_declares_its_dedicated_directory_trust_parser() {
    if require_plugins().is_none() {
        return;
    }
    assert!(!codex().supports_structured_prompts());
    assert!(codex().supports_directory_trust_prompt_parsing());
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
    let blocks: Vec<_> = delta
        .messages
        .iter()
        .flat_map(|m| m.blocks.iter())
        .collect();
    let kinds: Vec<&str> = blocks.iter().map(|b| b.kind.as_str()).collect();
    assert_eq!(
        kinds,
        vec!["thinking", "tool_use", "tool_use"],
        "{:#?}",
        delta.messages
    );
    assert_eq!(
        blocks[0].data,
        serde_json::json!({ "text": "**Inspect sources**" })
    );
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

#[test]
fn codex_declares_no_permission_auto_confirm() {
    if require_plugins().is_none() {
        return;
    }
    assert!(!codex().supports_permission_auto_confirm());
    assert!(!codex().describe().supports_permission_auto_confirm);
}
