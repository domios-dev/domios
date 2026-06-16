use super::*;
use crate::plugin::agent_codex::CodexProvider;

fn pair() -> (CodexProvider, JsProvider) {
    (CodexProvider::new(), load_js("codex"))
}

#[test]
fn detect_from_title_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (n, j) = pair();
    for s in ["Codex", "codex prompt", "RUNNING CODEX"] {
        assert!(n.detect_from_title(s));
        assert!(j.detect_from_title(s));
    }
    for s in ["bash", "claude"] {
        assert!(!n.detect_from_title(s));
        assert!(!j.detect_from_title(s));
    }
}

#[test]
fn detect_from_output_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (n, j) = pair();
    let cases = [
        ("OpenAI Codex v1.2", true),
        ("permissions: YOLO mode", true),
        ("Would you like to run the following command", true),
        ("Would you like to grant these permissions", true),
        ("approve network access", true),
        ("codex>", true),
        ("random shell text", false),
    ];
    for (s, expect) in cases {
        assert_eq!(n.detect_from_output(s), expect, "native: {s}");
        assert_eq!(j.detect_from_output(s), expect, "js: {s}");
    }
}

#[test]
fn screen_has_tui_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (n, j) = pair();
    for s in ["OpenAI Codex banner", "ctrl+c to cancel"] {
        assert!(n.screen_has_tui(s));
        assert!(j.screen_has_tui(s));
    }
    for s in ["bash $ ", "random output"] {
        assert!(!n.screen_has_tui(s));
        assert!(!j.screen_has_tui(s));
    }
}

#[test]
fn has_event_markers_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (n, j) = pair();
    for s in [
        "Would you like to proceed",
        "do you approve this",
        "run the following command?",
        "make the following edits?",
        "grant these permissions to",
    ] {
        assert_eq!(n.has_event_markers(s), j.has_event_markers(s), "{s}");
        assert!(n.has_event_markers(s));
    }
    assert_eq!(
        n.has_event_markers("just normal output"),
        j.has_event_markers("just normal output")
    );
}

#[test]
fn slash_commands_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (n, j) = pair();
    assert_eq!(n.compact_command(), j.compact_command());
    assert_eq!(n.exit_command(), j.exit_command());
    assert_eq!(n.auto_approve_flags(), j.auto_approve_flags());
    assert_eq!(
        n.model_launch_args("gpt-5.5"),
        j.model_launch_args("gpt-5.5")
    );
}

#[test]
fn parse_session_messages_parity_on_rollout_fixture() {
    if require_plugins().is_none() {
        return;
    }
    let (n, j) = pair();
    let jsonl = "{\"type\":\"meta\",\"x\":1}\n\
            {\"type\":\"response_item\",\"timestamp\":\"2026-04-12T09:00:00Z\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"hello\"}]}}\n\
            {\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"hi back\"},{\"type\":\"tool_call\"}]}}\n\
            {\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"developer\",\"content\":[{\"type\":\"text\",\"text\":\"skip me\"}]}}\n";

    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), jsonl).unwrap();

    let nm = n.parse_session_messages(tmp.path());
    let jm = j.parse_session_messages(tmp.path());

    assert_eq!(nm.len(), jm.len());
    for (a, b) in nm.iter().zip(jm.iter()) {
        assert_eq!(a.role, b.role);
        assert_eq!(a.text, b.text);
        assert_eq!(a.is_meta, b.is_meta);
    }
}

#[test]
fn metadata_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (n, j) = pair();
    assert_eq!(n.id(), j.id());
    assert_eq!(n.display_name(), j.display_name());
    assert_eq!(n.binary_name(), j.binary_name());
    assert_eq!(n.cli_binary(), j.cli_binary());
    assert_eq!(n.icon_hint(), j.icon_hint());
    assert_eq!(n.icon_color(), j.icon_color());
    assert_eq!(n.status_bar_rows(), j.status_bar_rows());
    assert_eq!(
        n.preserves_context_on_resume(),
        j.preserves_context_on_resume()
    );
}

#[test]
fn resume_command_carries_session_id() {
    if require_plugins().is_none() {
        return;
    }
    let (_n, j) = pair();
    let cmd = j.build_resume_command("uuid-1234", false).unwrap();
    assert_eq!(
        cmd,
        "codex resume --dangerously-bypass-hook-trust 'uuid-1234'"
    );
    let cmd2 = j.build_resume_command("uuid-1234", true).unwrap();
    assert_eq!(
            cmd2,
            "codex resume --dangerously-bypass-hook-trust --dangerously-bypass-approvals-and-sandbox 'uuid-1234'"
        );
}

/// Build a synthetic `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`
/// tree in a tempdir, then walk through the plugin. Verifies
/// enumerateSessions and sessionFilePath work end-to-end without native
/// Rust help, while session detection stays strict instead of guessing by
/// newest cwd rollout.
#[test]
fn codex_rollout_walk_via_plugin() {
    if require_plugins().is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    // Plugin honours `CODETERM_CODEX_SESSIONS_ROOT` to avoid hitting the user's real `~/.codex/sessions` during tests. (dirs::home_dir ignores
    // HOME env on Windows, so HOME redirect would be ineffective.)
    let sessions_root = dir.path().join("codex-sessions");
    let sessions = sessions_root.join("2026").join("04").join("12");
    std::fs::create_dir_all(&sessions).unwrap();

    let uuid_a = "019d773a-a5e8-7a02-9295-a40f9f9016ea";
    let uuid_b = "029d773a-a5e8-7a02-9295-a40f9f9016ea";
    let uuid_c = "039d773a-a5e8-7a02-9295-a40f9f9016ea";
    let file_a = sessions.join(format!("rollout-2026-04-12T10-00-01-{uuid_a}.jsonl"));
    let file_b = sessions.join(format!("rollout-2026-04-12T10-30-00-{uuid_b}.jsonl"));
    let file_c = sessions.join(format!("rollout-2026-04-12T11-00-00-{uuid_c}.jsonl"));
    std::fs::write(
        &file_a,
        format!(
            r#"{{"type":"session_meta","payload":{{"id":"{uuid_a}","cwd":"/proj/foo"}}}}
{{"type":"response_item","payload":{{"type":"message","role":"user","content":[]}}}}
"#
        ),
    )
    .unwrap();
    std::fs::write(
        &file_b,
        format!(
            r#"{{"type":"session_meta","payload":{{"id":"{uuid_b}","cwd":"/other/dir"}}}}
"#
        ),
    )
    .unwrap();
    std::fs::write(
        &file_c,
        format!(
            r#"{{"type":"session_meta","payload":{{"id":"{uuid_c}","cwd":"/proj/foo"}}}}
"#
        ),
    )
    .unwrap();
    // Touch modified times so c > a > b ordering is deterministic.
    let now = std::time::SystemTime::now();
    let _ = filetime::set_file_mtime(
        &file_a,
        filetime::FileTime::from_system_time(now - std::time::Duration::from_secs(60)),
    );
    let _ = filetime::set_file_mtime(&file_c, filetime::FileTime::from_system_time(now));

    // Synthetic plugin pointing at our tempdir via HOME env.
    let plugin_js = std::fs::read_to_string(
        workspace_plugins_root()
            .join("provider-codex")
            .join("plugin.js"),
    )
    .unwrap();
    let manifest_json = r#"{
            "id":"codex-test","version":"0","displayName":"X","binaryName":"codex",
            "spawn":{"readinessStrategy":{"kind":"fixed_delay","durationMs":0},
                     "taskDelivery":"pty_input","verifySubmit":{"kind":"none"}}
        }"#;
    let manifest: crate::plugin::js_runtime::PluginManifest =
        serde_json::from_str(manifest_json).unwrap();
    let prev = std::env::var("CODETERM_CODEX_SESSIONS_ROOT").ok();
    unsafe {
        std::env::set_var(
            "CODETERM_CODEX_SESSIONS_ROOT",
            sessions_root.to_string_lossy().as_ref(),
        );
    }
    let vm = crate::plugin::js_runtime::vm::PluginVm::from_source_with(
        "codex-test",
        plugin_js,
        Some(manifest_json.to_string()),
    )
    .unwrap();
    let provider = JsProvider::new(manifest, std::sync::Arc::new(vm));

    // enumerateSessions sees all three rollouts with their meta.
    let all = provider.enumerate_sessions();
    let by_id: std::collections::HashMap<_, _> = all
        .iter()
        .map(|s| (s.session_id.clone(), s.clone()))
        .collect();
    assert_eq!(
        by_id.len(),
        3,
        "expected 3 sessions, got {}: {by_id:#?}",
        by_id.len()
    );
    assert_eq!(by_id[uuid_a].provider, "codex");
    assert_eq!(by_id[uuid_a].project_path.as_deref(), Some("/proj/foo"));
    assert_eq!(by_id[uuid_b].project_path.as_deref(), Some("/other/dir"));

    assert_eq!(provider.detect_session_id("/proj/foo", &[], None), None);
    assert_eq!(provider.detect_active_session("/proj/foo", uuid_a), None);

    // sessionFilePath resolves UUID → on-disk rollout path.
    let path = provider.session_file_path("ignored", uuid_a).unwrap();
    assert_eq!(path, file_a);

    // Cleanup env so other tests see the original.
    unsafe {
        match prev {
            Some(v) => std::env::set_var("CODETERM_CODEX_SESSIONS_ROOT", v),
            None => std::env::remove_var("CODETERM_CODEX_SESSIONS_ROOT"),
        }
    }
}

#[test]
fn codex_session_detection_does_not_guess_newest_cwd_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (n, j) = pair();
    assert_eq!(n.detect_session_id("/same/cwd", &[], None), None);
    assert_eq!(j.detect_session_id("/same/cwd", &[], None), None);
    assert_eq!(n.detect_active_session("/same/cwd", "sid-a"), None);
    assert_eq!(j.detect_active_session("/same/cwd", "sid-a"), None);
}
