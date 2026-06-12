use super::*;

static OPENCODE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn opencode_provider_id() {
    let p = OpenCodeProvider::new();
    assert_eq!(p.id(), "opencode");
    assert_eq!(p.display_name(), "OpenCode");
    assert_eq!(p.binary_name(), "opencode");
}

#[test]
fn opencode_auto_approve_flags() {
    let p = OpenCodeProvider::new();
    assert_eq!(
        p.auto_approve_flags(),
        vec!["--dangerously-skip-permissions"]
    );
}

#[test]
fn opencode_detect_from_title() {
    let p = OpenCodeProvider::new();
    assert!(p.detect_from_title("opencode"));
    assert!(p.detect_from_title("OpenCode - project"));
    assert!(p.detect_from_title("* My OpenCode Session"));
    assert!(!p.detect_from_title("claude"));
    assert!(!p.detect_from_title("codex"));
}

#[test]
fn opencode_launch_interactive() {
    let p = OpenCodeProvider::new();
    let cmd = p.build_launch_command(&AgentLaunchParams {
        agent_name: None,
        task: None,
        args: None,
        skip_permissions: false,
        headless: false,
        session_id: None,
    });
    assert_eq!(cmd, "opencode");
}

#[test]
fn opencode_launch_interactive_skip_perms_does_not_attach_flag() {
    // The --dangerously-skip-permissions flag is `run`-only. Interactive TUI launches must NOT include it (opencode rejects unknown args).
    let p = OpenCodeProvider::new();
    let cmd = p.build_launch_command(&AgentLaunchParams {
        agent_name: None,
        task: None,
        args: None,
        skip_permissions: true,
        headless: false,
        session_id: None,
    });
    assert_eq!(cmd, "opencode");
    assert!(!cmd.contains("--dangerously-skip-permissions"));
}

#[test]
fn opencode_launch_with_task_interactive_pane_uses_tui() {
    let p = OpenCodeProvider::new();
    let cmd = p.build_launch_command(&AgentLaunchParams {
        agent_name: None,
        task: Some("fix the bug".to_string()),
        args: None,
        skip_permissions: true,
        headless: false,
        session_id: None,
    });
    assert_eq!(cmd, "opencode");
    assert!(!cmd.contains("run"));
    assert!(!cmd.contains("fix the bug"));
    assert!(matches!(
        p.task_delivery(),
        crate::agent::provider::TaskDelivery::PtyInput
    ));
}

#[test]
fn opencode_launch_with_task_headless_worker_uses_run() {
    let p = OpenCodeProvider::new();
    let cmd = p.build_launch_command(&AgentLaunchParams {
        agent_name: None,
        task: Some("fix the bug".to_string()),
        args: None,
        skip_permissions: true,
        headless: true,
        session_id: None,
    });
    assert!(cmd.starts_with("opencode run"));
    assert!(cmd.contains("--dangerously-skip-permissions"));
    assert!(cmd.contains("--format json"));
    assert!(cmd.contains("fix the bug"));
}

#[test]
fn opencode_launch_headless_without_task_falls_back_to_tui() {
    let p = OpenCodeProvider::new();
    let cmd = p.build_launch_command(&AgentLaunchParams {
        agent_name: None,
        task: None,
        args: None,
        skip_permissions: true,
        headless: true,
        session_id: None,
    });
    assert_eq!(cmd, "opencode");
}

#[test]
fn opencode_launch_interactive_with_extra_args() {
    let p = OpenCodeProvider::new();
    let cmd = p.build_launch_command(&AgentLaunchParams {
        agent_name: None,
        task: Some("hello".to_string()),
        args: Some(vec!["-m".into(), "anthropic/claude-opus-4-7".into()]),
        skip_permissions: true,
        headless: false,
        session_id: None,
    });
    assert_eq!(cmd, "opencode -m anthropic/claude-opus-4-7");
}

#[test]
fn opencode_resume_command_uses_session_flag() {
    let p = OpenCodeProvider::new();
    let cmd = p
        .build_resume_command("ses_3bc903c6bffeLofCwfS0EczYak", true)
        .unwrap();
    assert!(cmd.starts_with("opencode --session"));
    assert!(cmd.contains("ses_3bc903c6bffeLofCwfS0EczYak"));
    // TUI doesn't accept --dangerously-skip-permissions even when caller asks for skip — provider must drop it silently.
    assert!(!cmd.contains("--dangerously-skip-permissions"));
}

#[test]
fn opencode_compact_and_exit() {
    let p = OpenCodeProvider::new();
    assert_eq!(p.compact_command(), Some("/compact"));
    assert_eq!(p.exit_command(), Some("/exit"));
}

#[test]
fn opencode_event_detection() {
    let p = OpenCodeProvider::new();
    let events = p.detect_events("Permission required: Bash(rm -rf /)");
    assert_eq!(events.len(), 1);
    assert!(matches!(
        events[0],
        crate::agent::detector::AgentEvent::PermissionRequest
    ));

    let events = p.detect_events("Hello, how can I help?");
    assert!(events.is_empty());
}

#[test]
fn opencode_noise_detection() {
    let p = OpenCodeProvider::new();
    assert!(p.is_output_noise(""));
    assert!(p.is_output_noise("   "));
    assert!(p.is_output_noise("───────────"));
    assert!(p.is_output_noise("█▀▀█ █▀▀█ █▀▀█"));
    assert!(!p.is_output_noise("Some actual output"));
}

#[test]
fn opencode_status_bar_rows() {
    let p = OpenCodeProvider::new();
    assert_eq!(p.status_bar_rows(), 2);
}

#[test]
fn opencode_describe_assembles_full_descriptor() {
    let p = OpenCodeProvider::new();
    let desc = p.describe();
    assert_eq!(desc.id, "opencode");
    assert_eq!(desc.display_name, "OpenCode");
    assert_eq!(desc.icon_hint.as_deref(), Some("◇"));
    assert_eq!(
        desc.default_model.as_deref(),
        Some("anthropic/claude-opus-4-7")
    );
    assert!(desc.preserves_context_on_resume);
    assert!(!desc.has_hooks);
    assert!(!desc.settings_schema.is_empty());
}

#[test]
fn opencode_describe_omits_usage_tracking_section() {
    // OpenCode has no usage_parser — describe() must NOT inject the Usage Tracking toggle. The auto-injection in `describe()` keys off
    // `usage_parser().is_some()`, so this test pins that contract.
    let p = OpenCodeProvider::new();
    let desc = p.describe();
    let has = desc.settings_schema.iter().any(|f| {
        matches!(
            f,
            crate::agent::provider::SettingField::Section { title, .. } if title == "Usage Tracking"
        )
    });
    assert!(
        !has,
        "OpenCode has no usage_parser — Usage Tracking section must not appear"
    );
}

#[test]
fn opencode_settings_schema_has_required_sections() {
    let p = OpenCodeProvider::new();
    let schema = p.settings_schema();
    let titles: Vec<&str> = schema
        .iter()
        .filter_map(|f| match f {
            crate::agent::provider::SettingField::Section { title, .. } => Some(title.as_str()),
            _ => None,
        })
        .collect();
    assert!(titles.contains(&"Session Resume"));
    assert!(titles.contains(&"Model"));
    assert!(titles.contains(&"Requirements"));
}

#[test]
fn opencode_settings_schema_round_trips_as_json() {
    let p = OpenCodeProvider::new();
    let desc = p.describe();
    let json = serde_json::to_string(&desc).expect("serialize");
    let decoded: crate::agent::provider::ProviderDescriptor =
        serde_json::from_str(&json).expect("deserialize");
    assert_eq!(desc, decoded);
}

#[test]
fn opencode_does_not_contribute_notifications_card() {
    // Opencode notifications fire automatically via the PTY event pipeline — no user-facing setting to render. Keeps the Notifications tab free of
    // "auto-enabled, no setup required" noise.
    let p = OpenCodeProvider::new();
    assert!(p.notifications_schema().is_empty());
    assert!(p.describe().notifications_schema.is_empty());
}

#[test]
fn opencode_default_model_is_set() {
    let p = OpenCodeProvider::new();
    assert_eq!(
        p.default_model().as_deref(),
        Some("anthropic/claude-opus-4-7")
    );
}

#[test]
fn opencode_models_returns_non_empty_list_via_live_or_fallback() {
    // Either the live `opencode models` succeeds and we get >0 entries, or the fetch fails and the fallback seed list kicks in. Both paths must
    // produce a non-empty list — settings UI relies on it.
    let p = OpenCodeProvider::new();
    let models = p.available_models();
    assert!(!models.is_empty(), "available_models must never be empty");
    // Every entry must be a proper provider/model pair.
    for m in &models {
        assert!(
            m.id.contains('/'),
            "model id without provider prefix: {}",
            m.id
        );
    }
}

#[test]
fn opencode_fallback_seed_is_stable() {
    let seed = fallback_models();
    let ids: Vec<&str> = seed.iter().map(|m| m.id.as_str()).collect();
    assert!(ids.contains(&"anthropic/claude-opus-4-7"));
    assert!(ids.contains(&"anthropic/claude-sonnet-4-6"));
    assert!(ids.contains(&"opencode/big-pickle"));
}

#[test]
fn prettify_model_id_humanises_known_providers() {
    assert_eq!(
        prettify_model_id("anthropic/claude-opus-4-7"),
        "Anthropic · Claude Opus 4 7"
    );
    assert_eq!(
        prettify_model_id("github-copilot/gpt-5-mini"),
        "GitHub Copilot · Gpt 5 Mini"
    );
    assert_eq!(
        prettify_model_id("opencode/big-pickle"),
        "OpenCode · Big Pickle"
    );
    // Unknown provider — keep the raw segment so we don't lose info.
    assert_eq!(
        prettify_model_id("zoinks/super-fast"),
        "zoinks · Super Fast"
    );
    // Bare id without slash — return as-is, capitalised.
    assert_eq!(prettify_model_id("just-a-name"), "Just A Name");
}

#[test]
fn opencode_meta_filter_known_injections() {
    assert!(is_codeterm_injection(
        "# AGENTS.md instructions for /Users/x"
    ));
    assert!(is_codeterm_injection("<INSTRUCTIONS>\nYou are..."));
    assert!(is_codeterm_injection(
        "<environment_context>\n  <cwd>/x</cwd>"
    ));
    assert!(is_codeterm_injection(
        "You are the General Agent — the always-on system brain"
    ));
    assert!(!is_codeterm_injection("fix the bug in parser.rs"));
    assert!(!is_codeterm_injection("# TODO\n- ship v2"));
}

#[test]
fn opencode_session_id_injection_default_none() {
    // OpenCode generates session ids itself (`ses_*`) — no pre-launch injection like Claude's --session-id flag.
    let p = OpenCodeProvider::new();
    assert_eq!(
        p.session_id_injection(),
        crate::agent::provider::SessionIdInjection::None
    );
}

// ── Cross-provider non-contamination ───────────────────────────────

#[test]
fn opencode_does_not_claim_codex_permission_output() {
    let p = OpenCodeProvider::new();
    // Codex permission prompts — must NOT be claimed by opencode.
    assert!(!p.detect_from_output("Would you like to run the following command?\n$ ls -la"));
    assert!(
        !p.detect_from_output("Would you like to make the following edits?\ndiff --git a/x b/x")
    );
    assert!(!p.detect_from_output("Would you like to grant these permissions?\n  codex>"));
}

#[test]
fn opencode_detect_requires_opencode_marker() {
    let p = OpenCodeProvider::new();
    // "Permission required" alone (no "opencode" in text) must NOT match.
    assert!(!p.detect_from_output("Permission required: Bash(rm -rf /)"));
    // "approve" alone must NOT match — too generic, codex also uses it.
    assert!(!p.detect_from_output("Do you want to approve network access?"));
    // Only fires when "opencode" is present together with a permission marker.
    assert!(p.detect_from_output("opencode\nPermission required: Bash(ls)"));
    assert!(p.detect_from_output("opencode v0.3.0\n█▀▀█ █▀▀█"));
}

#[test]
fn opencode_approve_alone_does_not_fire() {
    // "opencode" + "approve" must NOT trigger opencode detection on its own — this is the cross-fire path where codex output mentioning "approve
    // network access" could appear alongside "opencode" in screen content from a mixed pane. Only codex-specific "approve" phrasing must NOT be claimed by opencode.
    let p = OpenCodeProvider::new();
    let mixed = "opencode v0.3.0\nDo you want to approve network access to api.example.com?";
    // This used to fire opencode (via "approve" clause) AND codex simultaneously, causing non-deterministic tab labeling. opencode must not claim
    // this.
    assert!(
        !p.detect_from_output(mixed),
        "opencode must not claim 'approve network access' — that is codex's phrase"
    );
}

#[test]
fn opencode_detect_from_title_does_not_claim_codex_title() {
    let p = OpenCodeProvider::new();
    assert!(!p.detect_from_title("codex"));
    assert!(!p.detect_from_title("Codex - my project"));
}

fn fixture_db() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("opencode.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT);
         CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT, time_created INTEGER, data TEXT);",
    )
    .unwrap();
    let msg = |id: &str, role: &str| {
        conn.execute(
            "INSERT INTO message (id, session_id, time_created, data) VALUES (?1, 'ses_x', 1, ?2)",
            rusqlite::params![id, format!("{{\"role\":\"{role}\"}}")],
        )
        .unwrap();
    };
    let part = |id: &str, mid: &str, ts: i64, data: &str| {
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, data) VALUES (?1, ?2, 'ses_x', ?3, ?4)",
            rusqlite::params![id, mid, ts, data],
        )
        .unwrap();
    };
    msg("m1", "user");
    msg("m2", "assistant");
    part("prt_1", "m1", 100, r#"{"type":"text","text":"hello"}"#);
    part(
        "prt_2",
        "m2",
        200,
        r#"{"type":"reasoning","text":"let me think"}"#,
    );
    part(
        "prt_3",
        "m2",
        210,
        r#"{"type":"tool","tool":"edit","state":{"status":"completed","input":{"filePath":"/x","oldString":"a","newString":"b"},"output":"ok"}}"#,
    );
    part("prt_4", "m2", 220, r#"{"type":"step-finish"}"#);
    part("prt_5", "m2", 230, r#"{"type":"text","text":"done"}"#);
    (dir, path)
}

fn fixture_data_home_db() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let opencode_dir = dir.path().join("opencode");
    std::fs::create_dir_all(&opencode_dir).unwrap();
    let path = opencode_dir.join("opencode.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE session (id TEXT PRIMARY KEY, directory TEXT, time_created INTEGER, time_archived INTEGER);
         CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT);",
    )
    .unwrap();
    (dir, path)
}

fn insert_opencode_session(
    conn: &rusqlite::Connection,
    session_id: &str,
    cwd: &str,
    created: i64,
    message_time: i64,
    model: Option<&str>,
) {
    conn.execute(
        "INSERT INTO session (id, directory, time_created, time_archived) VALUES (?1, ?2, ?3, NULL)",
        rusqlite::params![session_id, cwd, created],
    )
    .unwrap();
    let data = match model {
        Some(model) => format!(r#"{{"role":"assistant","modelID":"{model}"}}"#),
        None => r#"{"role":"user"}"#.to_string(),
    };
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, data) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![format!("msg_{session_id}"), session_id, message_time, data],
    )
    .unwrap();
}

#[test]
fn opencode_detect_session_model_reads_the_claimed_session_only() {
    let _guard = OPENCODE_ENV_LOCK.lock().unwrap();
    let (dir, path) = fixture_data_home_db();
    let conn = rusqlite::Connection::open(&path).unwrap();
    let cwd = "/tmp/model-fidelity";
    insert_opencode_session(
        &conn,
        "ses_deepseek",
        cwd,
        1,
        100,
        Some("opencode/deepseek-v4-flash"),
    );
    insert_opencode_session(
        &conn,
        "ses_bigpickle",
        cwd,
        2,
        200,
        Some("opencode/big-pickle"),
    );

    let prev = std::env::var("XDG_DATA_HOME").ok();
    unsafe {
        std::env::set_var("XDG_DATA_HOME", dir.path());
    }
    let p = OpenCodeProvider::new();
    assert_eq!(
        p.detect_session_model(cwd, "ses_deepseek").as_deref(),
        Some("Deepseek")
    );
    assert_eq!(
        p.detect_session_model(cwd, "ses_bigpickle").as_deref(),
        Some("big-pickle")
    );
    assert_eq!(p.detect_session_model(cwd, "ses_missing"), None);
    unsafe {
        match prev {
            Some(value) => std::env::set_var("XDG_DATA_HOME", value),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
    }
}

#[test]
fn opencode_detect_session_model_never_falls_back_to_global_latest() {
    let _guard = OPENCODE_ENV_LOCK.lock().unwrap();
    let (dir, path) = fixture_data_home_db();
    let conn = rusqlite::Connection::open(&path).unwrap();
    let cwd = "/tmp/model-fidelity";
    insert_opencode_session(&conn, "ses_empty", cwd, 1, 100, None);
    insert_opencode_session(
        &conn,
        "ses_global_latest",
        cwd,
        2,
        200,
        Some("opencode/big-pickle"),
    );

    let prev = std::env::var("XDG_DATA_HOME").ok();
    unsafe {
        std::env::set_var("XDG_DATA_HOME", dir.path());
    }
    let p = OpenCodeProvider::new();
    assert_eq!(p.detect_session_model(cwd, "ses_empty"), None);
    assert_eq!(p.detect_active_session(cwd, "ses_empty"), None);
    unsafe {
        match prev {
            Some(value) => std::env::set_var("XDG_DATA_HOME", value),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
    }
}

#[test]
fn opencode_uses_structured_chat() {
    assert!(OpenCodeProvider::new().uses_structured_chat());
}

#[test]
fn structured_chat_maps_parts_and_skips_noise() {
    let (_d, path) = fixture_db();
    let out = OpenCodeProvider::query_structured_chat(&path, "ses_x", 0);
    // text(user), reasoning, tool, text(assistant) — step-finish skipped.
    let kinds: Vec<&str> = out.messages.iter().map(|m| m.kind.as_str()).collect();
    assert_eq!(kinds, vec!["user", "thinking", "tool_call", "assistant"]);
    assert_eq!(out.messages[0].content, "hello");
    assert_eq!(out.messages[0].id, "prt_1");
    assert_eq!(out.messages[2].tool_name.as_deref(), Some("edit"));
    // Tool input/output flow through so the chat can render a diff.
    assert_eq!(
        out.messages[2]
            .tool_input
            .as_ref()
            .and_then(|v| v.get("oldString"))
            .and_then(|v| v.as_str()),
        Some("a")
    );
    assert_eq!(out.messages[2].tool_result.as_deref(), Some("ok"));
    assert_eq!(out.cursor.as_deref(), Some("230"));
}

#[test]
fn structured_chat_cursor_reads_inclusive_tail_only() {
    let (_d, path) = fixture_db();
    // Resume from the last tool part's ts: re-reads >= 210 (tool, text), not earlier.
    let out = OpenCodeProvider::query_structured_chat(&path, "ses_x", 210);
    let ids: Vec<&str> = out.messages.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, vec!["prt_3", "prt_5"]);
    assert_eq!(out.cursor.as_deref(), Some("230"));
}

#[test]
fn structured_chat_empty_when_session_unknown() {
    let (_d, path) = fixture_db();
    let out = OpenCodeProvider::query_structured_chat(&path, "ses_missing", 0);
    assert!(out.messages.is_empty());
}
