//! Claude parity: detection, metadata, slash commands, launch, resume,
//! session-jsonl path resolution, and settings-json integration hooks.

use super::super::*;
use super::*;

fn integration_options(precompact_memory_export: bool) -> crate::provider::IntegrationInstallOptions {
    crate::provider::IntegrationInstallOptions {
        consent: true,
        precompact_memory_export,
    }
}

#[test]
fn detect_from_title_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (n, j) = pair();
    let cases = [
        ("Claude Code", true),
        ("claude", true),
        ("✳ Claude Code", true),
        ("⠐ General coding session", true),
        ("Aider", false),
        ("PS C:\\>", false),
    ];
    for (input, expected) in cases {
        assert_eq!(n.detect_from_title(input), expected, "native: {input}");
        assert_eq!(j.detect_from_title(input), expected, "js: {input}");
    }
}

#[test]
fn detect_from_output_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (n, j) = pair();
    let cases = [
        ("Do you want to proceed?", true),
        ("Cogitated for 3s", true),
        ("Brewed for 12s", true),
        ("Reasoned for 5s · $0.02 spent", true),
        ("random text", false),
        ("$ ls -la", false),
    ];
    for (input, expected) in cases {
        assert_eq!(n.detect_from_output(input), expected, "native: {input}");
        assert_eq!(j.detect_from_output(input), expected, "js: {input}");
    }
}

#[test]
fn screen_has_tui_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (n, j) = pair();
    for s in ["? for shortcuts here", "ctrl+l help", "esc to interrupt"] {
        assert!(n.screen_has_tui(s), "native: {s}");
        assert!(j.screen_has_tui(s), "js: {s}");
    }
    for s in ["normal shell", "just a $ prompt"] {
        assert!(!n.screen_has_tui(s));
        assert!(!j.screen_has_tui(s));
    }
}

#[test]
fn is_output_noise_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (n, j) = pair();
    let noisy = [
        "bypass permissions on",
        "shift+tab to cycle",
        "Update available! Run:",
        "esc to interrupt",
        "? for shortcuts",
        "─────────",
        "═════ ═════",
        "",
    ];
    for s in noisy {
        assert!(n.is_output_noise(s), "native should drop: {s:?}");
        assert!(j.is_output_noise(s), "js should drop: {s:?}");
    }
    for s in ["meaningful output", "$ pwd", "Reading file foo.rs"] {
        assert!(!n.is_output_noise(s), "native shouldn't drop: {s:?}");
        assert!(!j.is_output_noise(s), "js shouldn't drop: {s:?}");
    }
}

#[test]
fn extract_session_name_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (n, j) = pair();
    let explicit = "  Session: vivid-bhabha-1234\n  cwd: /tmp\n";
    assert_eq!(
        n.extract_session_name(explicit).as_deref(),
        Some("vivid-bhabha-1234")
    );
    assert_eq!(
        j.extract_session_name(explicit).as_deref(),
        Some("vivid-bhabha-1234")
    );

    let bare = "Resuming previous session\n\n  brave-tiger-4242\n\nReady.";
    assert_eq!(
        n.extract_session_name(bare).as_deref(),
        Some("brave-tiger-4242")
    );
    assert_eq!(
        j.extract_session_name(bare).as_deref(),
        Some("brave-tiger-4242")
    );

    let prose = "we hit error-code-1234 while parsing — see error-code-5678";
    assert_eq!(n.extract_session_name(prose), None);
    assert_eq!(j.extract_session_name(prose), None);
}

#[test]
fn slash_commands_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (n, j) = pair();
    assert_eq!(n.compact_command(), j.compact_command());
    assert_eq!(n.exit_command(), j.exit_command());
    assert_eq!(
        n.model_switch_command("haiku"),
        j.model_switch_command("haiku")
    );
    assert_eq!(n.auto_approve_flags(), j.auto_approve_flags());
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
    assert_eq!(n.has_hooks(), j.has_hooks());
    assert_eq!(n.supports_lazy_skills(), j.supports_lazy_skills());
    assert_eq!(n.status_bar_rows(), j.status_bar_rows());
}

#[test]
fn agent_state_status_reconciliation_capability_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (n, j) = pair();
    assert!(n.supports_agent_state_status_reconciliation());
    assert_eq!(
        n.supports_agent_state_status_reconciliation(),
        j.supports_agent_state_status_reconciliation()
    );
}

#[test]
fn parse_session_messages_parity_on_jsonl_fixture() {
    if require_plugins().is_none() {
        return;
    }
    let (n, j) = pair();
    // Realistic Claude JSONL snippet: assistant with array content, user with string content, plus a non-message line that must be dropped by both impls.
    let jsonl = "{\"type\":\"summary\",\"x\":1}\n\
            {\"type\":\"user\",\"isMeta\":false,\"message\":{\"role\":\"user\",\"content\":\"please refactor this\"},\"timestamp\":\"2026-05-01T10:00:00Z\"}\n\
            {\"type\":\"assistant\",\"isMeta\":false,\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Sure, I'll refactor it.\"},{\"type\":\"tool_use\",\"id\":\"x\"}]}}\n\
            {\"type\":\"user\",\"isMeta\":true,\"message\":{\"role\":\"user\",\"content\":\"<system reminder>\"}}\n";

    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), jsonl).unwrap();

    let nm = n.parse_session_messages(tmp.path());
    let jm = j.parse_session_messages(tmp.path());

    assert_eq!(nm.len(), jm.len(), "message count must match");
    for (i, (a, b)) in nm.iter().zip(jm.iter()).enumerate() {
        assert_eq!(a.role, b.role, "role @ {i}");
        assert_eq!(a.text, b.text, "text @ {i}");
        assert_eq!(a.is_meta, b.is_meta, "is_meta @ {i}");
        assert_eq!(a.ts, b.ts, "ts @ {i}");
    }
}

#[test]
fn resume_command_matches_native_provider() {
    if require_plugins().is_none() {
        return;
    }
    let (n, j) = pair();
    assert_eq!(
        n.build_resume_command("abc-def-123-456", true),
        j.build_resume_command("abc-def-123-456", true)
    );
}

#[test]
fn integration_install_creates_all_four_hooks_with_markers() {
    if require_plugins().is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let provider = claude_in_temp(
        &dir.path().join("rule.md"),
        &dir.path().join("settings.json"),
    );

    // Before install: settings_exists=false.
    let pre = provider.check_integration();
    assert_eq!(pre["settings_exists"], false);
    assert_eq!(pre["hooks_installed"], false);

    // Install.
    provider
        .install_integration(7686, &integration_options(true))
        .expect("install ok");
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("settings.json")).unwrap())
            .unwrap();
    for key in ["Notification", "Stop", "PreCompact", "SessionStart"] {
        let arr = json["hooks"][key].as_array().expect(key);
        assert!(
            arr.iter().any(|e| {
                e["hooks"].as_array().map_or(false, |inner| {
                    inner.iter().any(|h| h["_source"] == "codeterm-hook-v3")
                })
            }),
            "missing v2 hook in {key}"
        );
        // Every Claude process routes through the URL and token injected by
        // the CodeTerm instance that owns that exact Tab.
        let any_cmd = arr
            .iter()
            .flat_map(|e| e["hooks"].as_array().cloned().unwrap_or_default())
            .find_map(|h| h["command"].as_str().map(String::from))
            .unwrap();
        assert!(
            any_cmd.contains("CODETERM_API_URL")
                && any_cmd.contains("CODETERM_API_TOKEN")
                && !any_cmd.contains("CODETERM_API_PORT")
                && !any_cmd.contains("NUL"),
            "{key} cmd: {any_cmd}"
        );
    }

    // checkIntegration now reports installed.
    let post = provider.check_integration();
    assert_eq!(post["settings_exists"], true);
    assert_eq!(post["hooks_installed"], true);
    assert_eq!(post["notification_hook"], true);
    assert_eq!(post["stop_hook"], true);
    assert_eq!(post["precompact_hook"], true);
    assert_eq!(post["session_start_hook"], true);
}

#[test]
fn integration_install_preserves_unrelated_settings_fields() {
    if require_plugins().is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("settings.json");
    let original = r#"{"theme":"dark","permissions":{"allow":["Bash"]},"hooks":{"Notification":[]}}"#;
    std::fs::write(&settings, original).unwrap();
    let provider = claude_in_temp(&dir.path().join("rule.md"), &settings);
    provider.install_integration(7686, &integration_options(true)).unwrap();
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    // Unrelated fields untouched.
    assert_eq!(json["theme"], "dark");
    assert_eq!(json["permissions"]["allow"][0], "Bash");
    assert_eq!(
        std::fs::read_to_string(settings.with_file_name("settings.json.codeterm-backup")).unwrap(),
        original
    );
    provider.restore_integration().unwrap();
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), original);
}

#[test]
fn integration_rejects_malformed_non_object_and_empty_json_without_side_effects() {
    if require_plugins().is_none() {
        return;
    }
    for (name, fixture) in [
        ("malformed", "{not-json"),
        ("array", "[]"),
        ("null", "null"),
        ("empty", ""),
    ] {
        let native_dir = tempfile::tempdir().unwrap();
        let native_settings = native_dir.path().join("settings.json");
        std::fs::write(&native_settings, fixture).unwrap();
        let native_result = crate::agent_claude::provider::integration::install_integration_at(
            &native_settings,
            &integration_options(false),
        );
        assert!(native_result.is_err(), "native accepted {name}");
        assert_eq!(std::fs::read_to_string(&native_settings).unwrap(), fixture);
        assert!(!native_dir.path().join("settings.json.codeterm-backup").exists());

        let js_dir = tempfile::tempdir().unwrap();
        let js_settings = js_dir.path().join("settings.json");
        std::fs::write(&js_settings, fixture).unwrap();
        let js = claude_in_temp(&js_dir.path().join("rule.md"), &js_settings);
        let js_result = js.install_integration(7686, &integration_options(false));
        assert!(js_result.is_err(), "JS accepted {name}");
        assert_eq!(std::fs::read_to_string(&js_settings).unwrap(), fixture);
        assert!(!js_dir.path().join("settings.json.codeterm-backup").exists());
    }
}

#[test]
fn integration_uninstall_rejects_invalid_json_shapes_without_side_effects() {
    if require_plugins().is_none() {
        return;
    }
    for (name, fixture) in [
        ("malformed", "{not-json"),
        ("non-object", "[]"),
        ("malformed hook array", r#"{"hooks":{"Stop":{}}}"#),
    ] {
        for use_js in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let settings = dir.path().join("settings.json");
            std::fs::write(&settings, fixture).unwrap();
            let result = if use_js {
                claude_in_temp(&dir.path().join("rule.md"), &settings).uninstall_integration()
            } else {
                crate::agent_claude::provider::integration::uninstall_integration_at(&settings)
            };
            assert!(
                result.is_err(),
                "{} accepted {name}",
                if use_js { "JS" } else { "native" }
            );
            assert_eq!(std::fs::read_to_string(&settings).unwrap(), fixture);
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        }
    }
}

#[test]
fn integration_backup_and_restore_preserve_exact_bytes_in_native_and_js() {
    if require_plugins().is_none() {
        return;
    }
    let original = "{\n  \"foreign\": true,\n  \"hooks\": {}\n}\n";
    for use_js in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        std::fs::write(&settings, original).unwrap();
        if use_js {
            let provider = claude_in_temp(&dir.path().join("rule.md"), &settings);
            provider.install_integration(7686, &integration_options(false)).unwrap();
            provider.restore_integration().unwrap();
        } else {
            crate::agent_claude::provider::integration::install_integration_at(
                &settings,
                &integration_options(false),
            )
            .unwrap();
            crate::agent_claude::provider::integration::restore_integration_at(&settings).unwrap();
        }
        assert_eq!(std::fs::read_to_string(&settings).unwrap(), original);
    }
}

#[test]
fn integration_install_replaces_stale_codeterm_entries_not_user_hooks() {
    if require_plugins().is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("settings.json");
    // Pre-existing settings with: a user hook (must survive), a stale codeterm v1 hook (must be removed), and an unrelated key.
    std::fs::write(
            &settings,
            r#"{
                "hooks":{
                    "Notification":[
                        {"matcher":"perm","hooks":[{"type":"command","command":"my-user-script","timeout":1}]},
                        {"matcher":"perm","hooks":[{"type":"command","command":"curl http://localhost:7686/api/tabs/$X/hook/permission","_source":"codeterm"}]}
                    ]
                }
            }"#,
        )
        .unwrap();
    let provider = claude_in_temp(&dir.path().join("rule.md"), &settings);
    provider.install_integration(7686, &integration_options(true)).unwrap();
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    let arr = json["hooks"]["Notification"].as_array().unwrap();
    // Three entries expected: user hook + new v2 hook. (Stale v1 dropped.)
    assert_eq!(arr.len(), 2);
    assert!(
        arr.iter().any(|e| {
            e["hooks"][0]["command"]
                .as_str()
                .map_or(false, |s| s.contains("my-user-script"))
        }),
        "user hook must survive"
    );
    assert!(
        arr.iter().all(|e| {
            e["hooks"][0]["_source"]
                .as_str()
                .map_or(true, |s| s != "codeterm")
        }),
        "v1 stale entry must be dropped"
    );
    assert!(arr
        .iter()
        .any(|e| { e["hooks"][0]["_source"] == "codeterm-hook-v3" }));
}

#[test]
fn integration_uninstall_removes_every_codeterm_hook() {
    if require_plugins().is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let provider = claude_in_temp(
        &dir.path().join("rule.md"),
        &dir.path().join("settings.json"),
    );
    provider.install_integration(7686, &integration_options(true)).unwrap();
    provider.uninstall_integration().unwrap();
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("settings.json")).unwrap())
            .unwrap();
    let n = json["hooks"]["Notification"].as_array().unwrap();
    let s = json["hooks"]["Stop"].as_array().unwrap();
    let pc = json["hooks"]["PreCompact"].as_array().unwrap();
    let ss = json["hooks"]["SessionStart"].as_array().unwrap();
    assert!(n
        .iter()
        .all(|e| e["hooks"][0]["_source"] != "codeterm-hook-v3"));
    assert!(s
        .iter()
        .all(|e| e["hooks"][0]["_source"] != "codeterm-hook-v3"));
    assert!(pc
        .iter()
        .all(|e| e["hooks"][0]["_source"] != "codeterm-hook-v3"));
    assert!(ss
        .iter()
        .all(|e| e["hooks"][0]["_source"] != "codeterm-hook-v3"));
}

#[test]
fn integration_ensure_on_startup_performs_no_write_before_consent() {
    if require_plugins().is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let provider = claude_in_temp(
        &dir.path().join("rule.md"),
        &dir.path().join("settings.json"),
    );
    provider.ensure_integration_on_startup(7686);
    assert!(!dir.path().join("settings.json").exists());
}

#[test]
fn precompact_memory_export_is_separately_opt_in() {
    if require_plugins().is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let provider = claude_in_temp(&dir.path().join("rule.md"), &path);
    provider
        .install_integration(7686, &integration_options(false))
        .unwrap();
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(json["hooks"]["PreCompact"]
        .as_array()
        .map_or(true, |hooks| hooks.is_empty()));
}

#[test]
fn integration_install_is_identical_across_codeterm_instances() {
    if require_plugins().is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let provider = claude_in_temp(&dir.path().join("rule.md"), &path);

    provider.install_integration(7685, &integration_options(true)).unwrap();
    let prod = std::fs::read_to_string(&path).unwrap();
    provider.install_integration(7686, &integration_options(true)).unwrap();
    let dev = std::fs::read_to_string(&path).unwrap();

    assert_eq!(prod, dev);
    assert!(dev.contains("CODETERM_API_URL"));
    assert!(dev.contains("CODETERM_API_TOKEN"));
    assert!(!dev.contains("CODETERM_API_PORT"));
    assert!(!dev.contains("> NUL"));
}

/// Bridge-rule install: native impl writes to `~/.claude/rules/codeterm.md`
/// via `dirs::home_dir()`. The JS plugin gets the SAME functional
/// guarantee using only `host.expandHome` + `host.writeFile`. Test
/// builds a synthetic plugin whose `rule.path` points into a temp
/// dir, then runs `installBridgeRule` through `JsProvider` (same
/// trait method native uses) and asserts the file landed.
#[test]
fn bridge_rule_install_via_host_writes_file() {
    if require_plugins().is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let rule_path = dir.path().join("rules").join("codeterm.md");
    let provider = claude_in_temp(&rule_path, &dir.path().join("settings.json"));

    provider
        .install_bridge_rule("PROTOCOL PAYLOAD v1")
        .expect("install ok");
    let written = std::fs::read_to_string(&rule_path).expect("file written");
    assert_eq!(written, "PROTOCOL PAYLOAD v1");
    assert!(
        rule_path.parent().unwrap().exists(),
        "host.writeFile must create parent dirs"
    );

    provider.uninstall_bridge_rule().expect("uninstall ok");
    assert!(!rule_path.exists(), "uninstall removes the file");
}

#[test]
fn launch_command_no_task_matches_native_shape() {
    if require_plugins().is_none() {
        return;
    }
    let (n, j) = pair();
    let params = AgentLaunchParams {
        skip_permissions: true,
        ..Default::default()
    };
    let nat = n.build_launch_command(&params);
    let js = j.build_launch_command(&params);
    // Same env preamble + same trailing claude invocation. The JS form uses cross-platform host primitives so it should match native byte-for-byte for the no-task case.
    assert_eq!(nat, js, "native vs js launch mismatch");
}

#[test]
fn launch_command_with_task_matches_native_shape() {
    if require_plugins().is_none() {
        return;
    }
    let (n, j) = pair();
    let params = AgentLaunchParams {
        task: Some("line one\n<location>/repo</location>\n'quotes' \"double\" $HOME `whoami` --flag\nюникод".into()),
        skip_permissions: true,
        ..Default::default()
    };
    let nat = n.build_launch_command(&params);
    let js = j.build_launch_command(&params);
    for cmd in [&nat, &js] {
        // Interactive launch only: no -p/stream-json, task delivered as a positional after `--`.
        assert!(!cmd.contains(" -p"), "no headless -p: {cmd}");
        assert!(!cmd.contains("--output-format"), "no stream-json: {cmd}");
        assert!(cmd.contains(" -- "), "task after separator: {cmd}");
        assert!(cmd.contains(".ct-task-"));
        assert!(cmd.contains("--append-system-prompt-file"));
        assert!(!cmd.contains("$(cat "));
        assert!(!cmd.contains("<location>") && !cmd.contains("whoami") && !cmd.contains("юникод"));
        assert!(cmd.contains("--dangerously-skip-permissions"));
    }
}

#[test]
fn launch_command_session_id_injected_in_both() {
    if require_plugins().is_none() {
        return;
    }
    let (n, j) = pair();
    // Interactive launch always injects --session-id, with or without a task (the old -p path that dropped it is gone).
    for params in [
        AgentLaunchParams {
            session_id: Some("my-uuid".into()),
            skip_permissions: true,
            ..Default::default()
        },
        AgentLaunchParams {
            task: Some("x".into()),
            session_id: Some("my-uuid".into()),
            skip_permissions: true,
            ..Default::default()
        },
    ] {
        for cmd in [
            n.build_launch_command(&params),
            j.build_launch_command(&params),
        ] {
            assert!(cmd.contains("--session-id"), "must inject: {cmd}");
            assert!(cmd.contains("my-uuid"));
        }
    }
}

#[test]
fn session_jsonl_path_resolves_via_plugin() {
    if require_plugins().is_none() {
        return;
    }
    let j = load_js("claude");
    let p = j.session_jsonl_path("/Users/foo/work/proj", "sess-abc-123");
    assert!(p.is_some(), "should return Some(path)");
    let p = p.unwrap();
    let s = p.to_string_lossy();
    assert!(s.contains("sess-abc-123.jsonl"), "missing session id: {s}");
    // Dash-encoded cwd — non-alphanumerics become '-'
    assert!(
        s.contains("-Users-foo-work-proj"),
        "dash-encoded cwd missing: {s}"
    );
    assert!(
        s.contains(".claude") && s.contains("projects"),
        "must hit ~/.claude/projects: {s}"
    );
}

#[test]
fn session_jsonl_path_returns_none_for_empty_sid() {
    if require_plugins().is_none() {
        return;
    }
    let j = load_js("claude");
    assert!(
        j.session_jsonl_path("/proj", "").is_none(),
        "empty session id must return None"
    );
}

#[test]
fn usage_parser_waits_for_complete_reset_blocks_in_both_implementations() {
    if require_plugins().is_none() {
        return;
    }
    let (native, js) = pair();
    let native_parser = native.usage_parser().expect("native Claude usage parser");
    let js_parser = js.usage_parser().expect("JS Claude usage parser");
    let incomplete = "Current session 100% usedResets 8:10p (Europe/Kiev)\n\
                      Current week (all models) 55% usedResets Jul 19, 8pm (Europe/Kiev)";
    assert!(native_parser.parse(incomplete).is_err());
    assert!(js_parser.parse(incomplete).is_err());

    let complete = "Current session 100% usedResets 8:10pm (Europe/Kiev)\n\
                    Current week (all models) 55% usedResets Jul 19, 8pm (Europe/Kiev)";
    for snapshot in [
        native_parser
            .parse(complete)
            .expect("native complete frame"),
        js_parser.parse(complete).expect("JS complete frame"),
    ] {
        assert_eq!(snapshot.session_pct, Some(100.0));
        assert_eq!(snapshot.weekly_pct, Some(55.0));
        assert!(snapshot.session_resets_at_ms.is_some());
        assert!(snapshot.weekly_resets_at_ms.is_some());
    }
}

/// #163: the tailer reads `session_jsonl_path`, so it must resolve to the
/// canonical project dir the agent actually writes to. Claude canonicalizes
/// the cwd (symlinks resolved) before encoding; if the host hands the JS
/// drop-in a raw symlinked cwd, the tailer watches a dir that never appears
/// (e.g. `/tmp` vs `/private/tmp` on macOS) and chat never streams live.
#[cfg(unix)]
#[test]
fn session_jsonl_path_canonicalizes_symlinked_cwd() {
    use std::os::unix::fs::symlink;
    if require_plugins().is_none() {
        return;
    }
    let base = std::env::temp_dir().join(format!("ct-w3-163-{}", std::process::id()));
    let real = base.join("real-target");
    let link = base.join("link-alias");
    std::fs::create_dir_all(&real).unwrap();
    let _ = std::fs::remove_file(&link);
    symlink(&real, &link).unwrap();

    let j = load_js("claude");
    let p = j
        .session_jsonl_path(link.to_str().unwrap(), "sess-xyz")
        .expect("should resolve a path");
    let s = p.to_string_lossy();
    // otherwise the tailer watches a directory claude never writes to.
    assert!(
        s.contains("real-target"),
        "path must encode the canonical symlink target: {s}"
    );
    assert!(
        !s.contains("link-alias"),
        "path must not encode the raw symlink alias: {s}"
    );

    std::fs::remove_dir_all(&base).ok();
}
