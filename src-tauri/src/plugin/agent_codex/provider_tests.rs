use super::*;

static CODEX_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn codex_provider_id() {
    let p = CodexProvider::new();
    assert_eq!(p.id(), "codex");
    assert_eq!(p.display_name(), "Codex");
    assert_eq!(p.binary_name(), "codex");
}

#[test]
fn codex_auto_approve_flags() {
    let p = CodexProvider::new();
    assert_eq!(
        p.auto_approve_flags(),
        vec!["--dangerously-bypass-approvals-and-sandbox"]
    );
}

#[test]
fn codex_detect_from_title() {
    let p = CodexProvider::new();
    assert!(p.detect_from_title("codex"));
    assert!(p.detect_from_title("Codex - project"));
    assert!(p.detect_from_title("* My Codex Session"));
    assert!(!p.detect_from_title("claude"));
    assert!(!p.detect_from_title("random terminal"));
}

#[test]
fn codex_detect_from_output() {
    let p = CodexProvider::new();
    assert!(p.detect_from_output("Would you like to run the following command?"));
    assert!(p.detect_from_output("Would you like to make the following edits?"));
    assert!(p.detect_from_output("Do you want to approve network access to \"api.example.com\"?"));
    assert!(p.detect_from_output("codex>"));
    assert!(!p.detect_from_output("Claude Code v2.0"));
    assert!(!p.detect_from_output("opencode v0.3.0\nPermission required"));
}

#[test]
fn codex_does_not_claim_opencode_output() {
    let p = CodexProvider::new();
    // OpenCode-specific output must NOT be claimed by codex.
    assert!(!p.detect_from_output("opencode\nPermission required: Bash(ls)"));
    assert!(!p.detect_from_output("opencode v0.3.0\n█▀▀█ █▀▀█"));
    assert!(!p.detect_from_output("Allow this tool call?\n opencode session ses_abc"));
}

#[test]
fn codex_does_not_claim_claude_output() {
    let p = CodexProvider::new();
    // Claude-specific output must NOT be claimed by codex.
    assert!(!p.detect_from_output("Claude Code\nDo you want to proceed?"));
    assert!(!p.detect_from_output("Cogitated for 5s · $0.03 spent"));
}

#[test]
fn codex_launch_interactive() {
    let p = CodexProvider::new();
    let cmd = p.build_launch_command(&AgentLaunchParams {
        agent_name: None,
        task: None,
        args: None,
        skip_permissions: false,
        headless: false,
        session_id: None,
    });
    assert_eq!(cmd, "codex");
}

#[test]
fn codex_launch_interactive_skip_perms() {
    let p = CodexProvider::new();
    let cmd = p.build_launch_command(&AgentLaunchParams {
        agent_name: None,
        task: None,
        args: None,
        skip_permissions: true,
        headless: false,
        session_id: None,
    });
    // Codex CLI removed --full-auto; the surviving skip-approvals flag is --dangerously-bypass-approvals-and-sandbox (same one codex exec uses).
    assert_eq!(cmd, "codex --dangerously-bypass-approvals-and-sandbox");
}

#[test]
fn codex_launch_with_task_interactive_pane_uses_tui() {
    let p = CodexProvider::new();
    let cmd = p.build_launch_command(&AgentLaunchParams {
        agent_name: None,
        task: Some("fix the bug".to_string()),
        args: None,
        skip_permissions: true,
        headless: false,
        session_id: None,
    });
    assert_eq!(cmd, "codex --dangerously-bypass-approvals-and-sandbox");
    assert!(!cmd.contains("exec"));
    assert!(!cmd.contains("fix the bug"));
    assert!(matches!(
        p.task_delivery(),
        crate::agent::provider::TaskDelivery::PtyInput
    ));
}

#[test]
fn codex_launch_with_task_headless_worker_uses_exec() {
    let p = CodexProvider::new();
    let cmd = p.build_launch_command(&AgentLaunchParams {
        agent_name: None,
        task: Some("fix the bug".to_string()),
        args: None,
        skip_permissions: true,
        headless: true,
        session_id: None,
    });
    assert!(cmd.contains("codex exec --dangerously-bypass-approvals-and-sandbox"));
    assert!(cmd.contains("shell_environment_policy.inherit=all"));
    assert!(cmd.contains("--skip-git-repo-check"));
    assert!(cmd.contains("fix the bug"));
}

#[test]
fn codex_launch_headless_without_task_falls_back_to_tui() {
    let p = CodexProvider::new();
    let cmd = p.build_launch_command(&AgentLaunchParams {
        agent_name: None,
        task: None,
        args: None,
        skip_permissions: true,
        headless: true,
        session_id: None,
    });
    assert_eq!(cmd, "codex --dangerously-bypass-approvals-and-sandbox");
}

#[test]
fn codex_resume_command_with_uuid() {
    let p = CodexProvider::new();
    let cmd = p
        .build_resume_command("019d6240-e61f-7463-9d7d-6d5afa4e7f93", true)
        .unwrap();
    assert!(cmd.contains("codex resume"));
    assert!(cmd.contains("--dangerously-bypass-approvals-and-sandbox"));
    assert!(cmd.contains("019d6240-e61f-7463-9d7d-6d5afa4e7f93"));
}

#[test]
fn codex_resume_command_extracts_uuid_from_stem() {
    let p = CodexProvider::new();
    let cmd = p
        .build_resume_command(
            "rollout-2026-04-06T13-05-13-019d6240-e61f-7463-9d7d-6d5afa4e7f93",
            true,
        )
        .unwrap();
    assert!(cmd.contains("019d6240-e61f-7463-9d7d-6d5afa4e7f93"));
    assert!(!cmd.contains("rollout-"));
}

#[test]
fn extract_uuid_from_stem() {
    assert_eq!(
        extract_session_uuid("rollout-2026-04-06T13-05-13-019d6240-e61f-7463-9d7d-6d5afa4e7f93"),
        Some("019d6240-e61f-7463-9d7d-6d5afa4e7f93")
    );
    assert_eq!(extract_session_uuid("not-a-rollout"), None);
    assert_eq!(extract_session_uuid("rollout-short"), None);
}

#[test]
fn codex_fresh_detection_does_not_guess_newest_same_cwd_rollout() {
    let _guard = CODEX_ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let sessions = dir.path().join("2026").join("06").join("13");
    std::fs::create_dir_all(&sessions).unwrap();
    let cwd = "/tmp/codex-same-cwd";
    let sid_a = "019ebbbb-0000-7000-8000-000000000101";
    let sid_b = "019ebbbb-0000-7000-8000-000000000202";
    std::fs::write(
        sessions.join(format!("rollout-2026-06-13T12-00-00-{sid_a}.jsonl")),
        format!(r#"{{"type":"session_meta","payload":{{"id":"{sid_a}","cwd":"{cwd}"}}}}"#),
    )
    .unwrap();
    std::fs::write(
        sessions.join(format!("rollout-2026-06-13T12-00-01-{sid_b}.jsonl")),
        format!(r#"{{"type":"session_meta","payload":{{"id":"{sid_b}","cwd":"{cwd}"}}}}"#),
    )
    .unwrap();

    let prev = std::env::var("CODETERM_CODEX_SESSIONS_ROOT").ok();
    unsafe {
        std::env::set_var(
            "CODETERM_CODEX_SESSIONS_ROOT",
            dir.path().to_string_lossy().as_ref(),
        );
    }
    let p = CodexProvider::new();
    assert_eq!(p.detect_session_id(cwd, &[], None), None);
    assert_eq!(
        p.detect_launched_session_id(cwd, &[], Some(std::time::Duration::from_secs(300)), None),
        None
    );
    assert_eq!(
        p.detect_started_session(
            cwd,
            &[],
            Some(std::time::Duration::from_secs(300)),
            None,
            Some("OpenAI Codex\nSession ID: 019ebbbb-0000-7000-8000-000000000101"),
        )
        .as_deref(),
        Some(sid_a)
    );
    assert_eq!(
        p.detect_started_session(
            cwd,
            &[sid_a.to_string()],
            Some(std::time::Duration::from_secs(300)),
            None,
            Some("OpenAI Codex\nSession ID: 019ebbbb-0000-7000-8000-000000000101"),
        ),
        None
    );
    assert_eq!(
        p.detect_started_session(
            cwd,
            &[sid_a.to_string()],
            Some(std::time::Duration::from_secs(300)),
            None,
            Some("OpenAI Codex\nSession ID: 019ebbbb-0000-7000-8000-000000000202"),
        )
        .as_deref(),
        Some(sid_b)
    );
    unsafe {
        match prev {
            Some(value) => std::env::set_var("CODETERM_CODEX_SESSIONS_ROOT", value),
            None => std::env::remove_var("CODETERM_CODEX_SESSIONS_ROOT"),
        }
    }
}

#[test]
fn codex_detect_active_session_does_not_roll_to_foreign_newer_session() {
    let _guard = CODEX_ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let sessions = dir.path().join("2026").join("06").join("06");
    std::fs::create_dir_all(&sessions).unwrap();
    let cwd = "/Users/rollacode/.codeterm-dev/general";
    let old_sid = "019e1b8c-6ff3-7ec1-8088-856525752ce7";
    let new_sid = "019e9e33-c35c-7cd0-98e8-4e7aa7a7f537";
    let old_file = sessions.join(format!("rollout-2026-05-12T12-37-29-{old_sid}.jsonl"));
    let new_file = sessions.join(format!("rollout-2026-06-06T21-30-53-{new_sid}.jsonl"));
    std::fs::write(
        &old_file,
        format!(
            r#"{{"timestamp":"2026-05-12T09:41:43.084Z","type":"session_meta","payload":{{"id":"{old_sid}","cwd":"{cwd}"}}}}
{{"timestamp":"2026-05-12T09:41:43.090Z","type":"response_item","payload":{{"type":"message","role":"user","content":[{{"type":"input_text","text":"echo BANANA-7"}}]}}}}
"#
        ),
    )
    .unwrap();
    std::fs::write(
        &new_file,
        format!(
            r#"{{"timestamp":"2026-06-06T18:30:54.000Z","type":"session_meta","payload":{{"id":"{new_sid}","cwd":"{cwd}"}}}}
{{"timestamp":"2026-06-06T18:31:10.207Z","type":"response_item","payload":{{"type":"message","role":"user","content":[{{"type":"input_text","text":"Йо?"}}]}}}}
"#
        ),
    )
    .unwrap();
    let base = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000);
    filetime::set_file_mtime(&old_file, filetime::FileTime::from_system_time(base)).unwrap();
    filetime::set_file_mtime(
        &new_file,
        filetime::FileTime::from_system_time(base + std::time::Duration::from_secs(60)),
    )
    .unwrap();

    let prev = std::env::var("CODETERM_CODEX_SESSIONS_ROOT").ok();
    unsafe {
        std::env::set_var(
            "CODETERM_CODEX_SESSIONS_ROOT",
            dir.path().to_string_lossy().as_ref(),
        );
    }
    let p = CodexProvider::new();
    assert_eq!(
        p.session_file_path(cwd, old_sid).as_deref(),
        Some(old_file.as_path())
    );
    assert_eq!(p.detect_active_session(cwd, old_sid), None);
    assert_eq!(p.detect_active_session(cwd, new_sid), None);
    unsafe {
        match prev {
            Some(value) => std::env::set_var("CODETERM_CODEX_SESSIONS_ROOT", value),
            None => std::env::remove_var("CODETERM_CODEX_SESSIONS_ROOT"),
        }
    }
}

#[test]
fn codex_detect_session_model_reads_the_claimed_session_only() {
    let _guard = CODEX_ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let sessions = dir.path().join("2026").join("06").join("12");
    std::fs::create_dir_all(&sessions).unwrap();
    let cwd = "/tmp/model-fidelity";
    let sid_a = "019ebbbb-0000-7000-8000-000000000001";
    let sid_b = "019ebbbb-0000-7000-8000-000000000002";
    std::fs::write(
        sessions.join(format!("rollout-2026-06-12T12-00-00-{sid_a}.jsonl")),
        format!(
            r#"{{"type":"session_meta","payload":{{"id":"{sid_a}","cwd":"{cwd}"}}}}
{{"type":"turn_context","payload":{{"model":"gpt-5.4-mini"}}}}
"#
        ),
    )
    .unwrap();
    std::fs::write(
        sessions.join(format!("rollout-2026-06-12T12-00-01-{sid_b}.jsonl")),
        format!(
            r#"{{"type":"session_meta","payload":{{"id":"{sid_b}","cwd":"{cwd}"}}}}
{{"type":"turn_context","payload":{{"model":"gpt-5.5"}}}}
"#
        ),
    )
    .unwrap();

    let prev = std::env::var("CODETERM_CODEX_SESSIONS_ROOT").ok();
    unsafe {
        std::env::set_var(
            "CODETERM_CODEX_SESSIONS_ROOT",
            dir.path().to_string_lossy().as_ref(),
        );
    }
    let p = CodexProvider::new();
    assert_eq!(
        p.detect_session_model(cwd, sid_a).as_deref(),
        Some("GPT-5.4-mini")
    );
    assert_eq!(
        p.detect_session_model(cwd, sid_b).as_deref(),
        Some("GPT-5.5")
    );
    assert_eq!(p.detect_session_model(cwd, "missing-session"), None);
    assert_eq!(p.detect_active_session(cwd, sid_a), None);
    unsafe {
        match prev {
            Some(value) => std::env::set_var("CODETERM_CODEX_SESSIONS_ROOT", value),
            None => std::env::remove_var("CODETERM_CODEX_SESSIONS_ROOT"),
        }
    }
}

#[test]
fn codex_compact_and_exit() {
    let p = CodexProvider::new();
    assert_eq!(p.compact_command(), Some("/compact"));
    assert_eq!(p.exit_command(), Some("/exit"));
    assert_eq!(p.model_switch_command("gpt-5"), None);
}

#[test]
fn codex_event_detection() {
    let p = CodexProvider::new();
    let events = p.detect_events("Would you like to run the following command?\n$ ls -la");
    assert_eq!(events.len(), 1);
    assert!(matches!(
        events[0],
        crate::agent::detector::AgentEvent::PermissionRequest
    ));

    let events = p.detect_events("Hello, how can I help?");
    assert!(events.is_empty());
}

#[test]
fn codex_noise_detection() {
    let p = CodexProvider::new();
    assert!(p.is_output_noise(""));
    assert!(p.is_output_noise("   "));
    assert!(p.is_output_noise("───────────"));
    assert!(!p.is_output_noise("Some actual output"));
}

#[test]
fn codex_status_bar_rows() {
    let p = CodexProvider::new();
    assert_eq!(p.status_bar_rows(), 1);
}

// --- Self-presentation metadata (B.2) ---

#[test]
fn codex_metadata_describes_models() {
    let p = CodexProvider::new();
    let models = p.available_models();
    assert_eq!(models.len(), 5);
    let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
    assert!(ids.contains(&"gpt-5.5"));
    assert!(ids.contains(&"gpt-5.4"));
    assert!(ids.contains(&"gpt-5.4-mini"));
    assert!(ids.contains(&"gpt-5.3-codex"));
    assert!(ids.contains(&"gpt-5.2"));
    assert_eq!(p.default_model().as_deref(), Some("gpt-5.5"));
}

#[test]
fn codex_describe_assembles_full_descriptor() {
    let p = CodexProvider::new();
    let desc = p.describe();
    assert_eq!(desc.id, "codex");
    assert_eq!(desc.display_name, "Codex");
    assert_eq!(desc.icon_hint.as_deref(), Some("◆"));
    assert!(!desc.preserves_context_on_resume); // Codex resumes fresh
    assert!(!desc.has_hooks);
    assert!(!desc.settings_schema.is_empty());
}

#[test]
fn codex_settings_schema_has_required_sections() {
    let p = CodexProvider::new();
    let schema = p.settings_schema();
    let section_titles: Vec<&str> = schema
        .iter()
        .filter_map(|f| match f {
            crate::agent::provider::SettingField::Section { title, .. } => Some(title.as_str()),
            _ => None,
        })
        .collect();
    assert!(section_titles.contains(&"Session Resume"));
    assert!(section_titles.contains(&"Model"));
    assert!(section_titles.contains(&"Requirements"));
}

#[test]
fn codex_settings_schema_round_trips_as_json() {
    let p = CodexProvider::new();
    let desc = p.describe();
    let json = serde_json::to_string(&desc).expect("serialize");
    let decoded: crate::agent::provider::ProviderDescriptor =
        serde_json::from_str(&json).expect("deserialize");
    assert_eq!(desc, decoded);
}

#[test]
fn codex_does_not_contribute_notifications_card() {
    // Codex notifications fire automatically via the PTY event pipeline — no user-facing setting to render. Keeping the Notifications tab free of
    // "auto-enabled, no setup required" cards keeps the UI honest.
    let p = CodexProvider::new();
    assert!(p.notifications_schema().is_empty());
    assert!(p.describe().notifications_schema.is_empty());
}

// Meta filter for CodeTerm bootstrap lines injected into codex sessions.
// Tag them is_meta so search can ignore its own bootstrap noise.
#[test]
fn is_codeterm_injection_matches_known_boot_prompts() {
    assert!(is_codeterm_injection(
        "# AGENTS.md instructions for /Users/x/proj"
    ));
    assert!(is_codeterm_injection("<INSTRUCTIONS>\nYou are..."));
    assert!(is_codeterm_injection(
        "<environment_context>\n  <cwd>/x</cwd>\n</environment_context>"
    ));
    assert!(is_codeterm_injection(
        "<permissions instructions>\nFilesystem sandboxing..."
    ));
    assert!(is_codeterm_injection(
        "You are a Graph Worker for the CodeTerm memory system."
    ));
    assert!(is_codeterm_injection(
        "You are the General Agent — the always-on system brain"
    ));
    // Leading whitespace is ok — real codex payloads don't trim.
    assert!(is_codeterm_injection("   \n<INSTRUCTIONS>payload"));
}

#[test]
fn is_codeterm_injection_leaves_real_user_messages_alone() {
    assert!(!is_codeterm_injection("fix the bug in parser.rs"));
    assert!(!is_codeterm_injection("why did the deploy fail?"));
    assert!(!is_codeterm_injection("# TODO list\n- ship v2"));
    // User could paste the literal strings inside a question — that's legitimate. Strict `starts_with` keeps false positives low.
    assert!(!is_codeterm_injection(
        "looking at <INSTRUCTIONS> — what does it mean"
    ));
}

#[test]
fn extract_session_uuid_strips_rollout_timestamp_prefix() {
    // Real rollout stem layout: rollout-YYYY-MM-DDTHH-MM-SS-<uuid>
    let stem = "rollout-2026-04-10T14-50-25-019d773a-a5e8-7a02-9295-a40f9f9016ea";
    assert_eq!(
        extract_session_uuid(stem),
        Some("019d773a-a5e8-7a02-9295-a40f9f9016ea"),
    );
    // Malformed input must return None, not a garbled slice.
    assert_eq!(extract_session_uuid("not-a-rollout-file"), None);
    assert_eq!(extract_session_uuid("rollout-short"), None);
}
