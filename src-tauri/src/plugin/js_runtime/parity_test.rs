//! Parity tests: load each bundled JS plugin and assert its output
//! matches the native Rust provider on identical inputs for the
//! pure-logic surface (detection, slash commands, schemas, simple
//! launch/resume builds, JSONL parsing where applicable).
//!
//! These tests document exactly *which* AgentProvider methods are
//! safe to migrate to JS today. Methods that diverge (e.g. Claude's
//! stdin-pipe headless launch, which uses platform-specific shell
//! quoting + temp-file gymnastics that legitimately live in Rust)
//! are intentionally not covered here — the native provider remains
//! authoritative for them.

use std::path::PathBuf;

use crate::agent::provider::{AgentLaunchParams, AgentProvider};

use super::{load_one, JsProvider};

fn workspace_plugins_root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/..")).join("plugins")
}

fn ws_exists() -> bool {
    workspace_plugins_root().exists()
}

fn load_js(id: &str) -> JsProvider {
    let manifest = workspace_plugins_root()
        .join(format!("provider-{id}"))
        .join("plugin.json");
    load_one(&manifest).unwrap_or_else(|e| panic!("load {id} plugin: {e}"))
}

// ────────────────────────────────────────────────────────────────────
// Claude parity
// ────────────────────────────────────────────────────────────────────

mod claude {
    use super::*;
    use crate::plugin::agent_claude::provider::ClaudeProvider;

    fn pair() -> (ClaudeProvider, JsProvider) {
        (ClaudeProvider::new(), load_js("claude"))
    }

    #[test]
    fn detect_from_title_parity() {
        if !ws_exists() { return; }
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
        if !ws_exists() { return; }
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
        if !ws_exists() { return; }
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
        if !ws_exists() { return; }
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
        if !ws_exists() { return; }
        let (n, j) = pair();
        let explicit = "  Session: vivid-bhabha-1234\n  cwd: /tmp\n";
        assert_eq!(n.extract_session_name(explicit).as_deref(), Some("vivid-bhabha-1234"));
        assert_eq!(j.extract_session_name(explicit).as_deref(), Some("vivid-bhabha-1234"));

        let bare = "Resuming previous session\n\n  brave-tiger-4242\n\nReady.";
        assert_eq!(n.extract_session_name(bare).as_deref(), Some("brave-tiger-4242"));
        assert_eq!(j.extract_session_name(bare).as_deref(), Some("brave-tiger-4242"));

        let prose = "we hit error-code-1234 while parsing — see error-code-5678";
        assert_eq!(n.extract_session_name(prose), None);
        assert_eq!(j.extract_session_name(prose), None);
    }

    #[test]
    fn slash_commands_parity() {
        if !ws_exists() { return; }
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
        if !ws_exists() { return; }
        let (n, j) = pair();
        assert_eq!(n.id(), j.id());
        assert_eq!(n.display_name(), j.display_name());
        assert_eq!(n.binary_name(), j.binary_name());
        assert_eq!(n.cli_binary(), j.cli_binary());
        assert_eq!(n.icon_hint(), j.icon_hint());
        assert_eq!(n.icon_color(), j.icon_color());
        assert_eq!(n.preserves_context_on_resume(), j.preserves_context_on_resume());
        assert_eq!(n.has_hooks(), j.has_hooks());
        assert_eq!(n.supports_lazy_skills(), j.supports_lazy_skills());
        assert_eq!(n.status_bar_rows(), j.status_bar_rows());
    }

    #[test]
    fn parse_session_messages_parity_on_jsonl_fixture() {
        if !ws_exists() { return; }
        let (n, j) = pair();
        // Realistic Claude JSONL snippet: assistant with array content,
        // user with string content, plus a non-message line that must
        // be dropped by both impls.
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
    fn resume_command_carries_session_id_and_skip_flag() {
        if !ws_exists() { return; }
        let (_n, j) = pair();
        let cmd = j.build_resume_command("abc-def-123-456", true).unwrap();
        assert!(cmd.contains("--resume"));
        assert!(cmd.contains("abc-def-123-456"));
        assert!(cmd.contains("--dangerously-skip-permissions"));
    }

    /// Synthetic Claude plugin variant that points its rule + integration
    /// paths into a tempdir so we exercise the real plugin.js end-to-end
    /// against `JsProvider` without touching the user's real `~/.claude/`.
    fn claude_in_temp(
        rule_path: &std::path::Path,
        settings_path: &std::path::Path,
    ) -> JsProvider {
        let plugin_js = std::fs::read_to_string(
            workspace_plugins_root().join("provider-claude").join("plugin.js"),
        )
        .unwrap();
        let manifest_json = format!(
            r#"{{
                "id":"claude-test","version":"0.1.0","displayName":"X",
                "spawn":{{"readinessStrategy":{{"kind":"fixed_delay","durationMs":0}},
                         "taskDelivery":"pty_input",
                         "verifySubmit":{{"kind":"none"}}}},
                "rule":{{"path":"{rp}"}},
                "integration":{{"kind":"claude_settings_json","settingsPath":"{sp}"}}
            }}"#,
            rp = rule_path.to_string_lossy().replace('\\', "/"),
            sp = settings_path.to_string_lossy().replace('\\', "/"),
        );
        let manifest: crate::plugin::js_runtime::PluginManifest =
            serde_json::from_str(&manifest_json).unwrap();
        let vm = crate::plugin::js_runtime::vm::PluginVm::from_source_with(
            "claude-test",
            plugin_js,
            Some(manifest_json),
        )
        .unwrap();
        JsProvider::new(manifest, std::sync::Arc::new(vm))
    }

    #[test]
    fn integration_install_creates_all_four_hooks_with_markers() {
        if !ws_exists() { return; }
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
        provider.install_integration(7686).expect("install ok");
        let json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("settings.json")).unwrap(),
        )
        .unwrap();
        for key in ["Notification", "Stop", "PreCompact", "SessionStart"] {
            let arr = json["hooks"][key].as_array().expect(key);
            assert!(arr.iter().any(|e| {
                e["hooks"].as_array().map_or(false, |inner| {
                    inner.iter().any(|h| h["_source"] == "codeterm-hook-v2")
                })
            }), "missing v2 hook in {key}");
            // Curl points to the right port + path.
            let any_cmd = arr.iter().flat_map(|e| {
                e["hooks"].as_array().cloned().unwrap_or_default()
            }).find_map(|h| h["command"].as_str().map(String::from)).unwrap();
            assert!(any_cmd.contains("CODETERM_API_PORT:-7686"), "{key} cmd: {any_cmd}");
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
        if !ws_exists() { return; }
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        std::fs::write(
            &settings,
            r#"{"theme":"dark","permissions":{"allow":["Bash"]},"hooks":{"Notification":[]}}"#,
        )
        .unwrap();
        let provider = claude_in_temp(&dir.path().join("rule.md"), &settings);
        provider.install_integration(7686).unwrap();
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        // Unrelated fields untouched.
        assert_eq!(json["theme"], "dark");
        assert_eq!(json["permissions"]["allow"][0], "Bash");
    }

    #[test]
    fn integration_install_replaces_stale_codeterm_entries_not_user_hooks() {
        if !ws_exists() { return; }
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        // Pre-existing settings with: a user hook (must survive), a
        // stale codeterm v1 hook (must be removed), and an unrelated key.
        std::fs::write(
            &settings,
            r#"{
                "hooks":{
                    "Notification":[
                        {"matcher":"perm","hooks":[{"type":"command","command":"my-user-script","timeout":1}]},
                        {"matcher":"perm","hooks":[{"type":"command","command":"curl http://localhost:7686/api/panes/$X/hook/permission","_source":"codeterm"}]}
                    ]
                }
            }"#,
        )
        .unwrap();
        let provider = claude_in_temp(&dir.path().join("rule.md"), &settings);
        provider.install_integration(7686).unwrap();
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
                e["hooks"][0]["_source"].as_str().map_or(true, |s| s != "codeterm")
            }),
            "v1 stale entry must be dropped"
        );
        assert!(arr.iter().any(|e| {
            e["hooks"][0]["_source"] == "codeterm-hook-v2"
        }));
    }

    #[test]
    fn integration_uninstall_drops_notification_and_stop_keeps_precompact() {
        if !ws_exists() { return; }
        let dir = tempfile::tempdir().unwrap();
        let provider = claude_in_temp(
            &dir.path().join("rule.md"),
            &dir.path().join("settings.json"),
        );
        provider.install_integration(7686).unwrap();
        provider.uninstall_integration().unwrap();
        let json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("settings.json")).unwrap(),
        )
        .unwrap();
        // Mirroring native: Notification + Stop dropped; PreCompact +
        // SessionStart kept (mem continuity).
        let n = json["hooks"]["Notification"].as_array().unwrap();
        let s = json["hooks"]["Stop"].as_array().unwrap();
        let pc = json["hooks"]["PreCompact"].as_array().unwrap();
        let ss = json["hooks"]["SessionStart"].as_array().unwrap();
        assert!(n.iter().all(|e| e["hooks"][0]["_source"] != "codeterm-hook-v2"));
        assert!(s.iter().all(|e| e["hooks"][0]["_source"] != "codeterm-hook-v2"));
        assert!(pc.iter().any(|e| e["hooks"][0]["_source"] == "codeterm-hook-v2"));
        assert!(ss.iter().any(|e| e["hooks"][0]["_source"] == "codeterm-hook-v2"));
    }

    #[test]
    fn integration_ensure_on_startup_is_idempotent() {
        if !ws_exists() { return; }
        let dir = tempfile::tempdir().unwrap();
        let provider = claude_in_temp(
            &dir.path().join("rule.md"),
            &dir.path().join("settings.json"),
        );
        provider.ensure_integration_on_startup(7686);
        let first = std::fs::read_to_string(dir.path().join("settings.json")).unwrap();
        provider.ensure_integration_on_startup(7686);
        let second = std::fs::read_to_string(dir.path().join("settings.json")).unwrap();
        // Second pass must NOT duplicate hooks — checkIntegration sees
        // installed=true and skips re-write.
        assert_eq!(first, second);
    }

    /// Bridge-rule install: native impl writes to `~/.claude/rules/codeterm.md`
    /// via `dirs::home_dir()`. The JS plugin gets the SAME functional
    /// guarantee using only `host.expandHome` + `host.writeFile`. Test
    /// builds a synthetic plugin whose `rule.path` points into a temp
    /// dir, then runs `installBridgeRule` through `JsProvider` (same
    /// trait method native uses) and asserts the file landed.
    #[test]
    fn bridge_rule_install_via_host_writes_file() {
        if !ws_exists() { return; }
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
        if !ws_exists() { return; }
        let (n, j) = pair();
        let params = AgentLaunchParams {
            skip_permissions: true,
            ..Default::default()
        };
        let nat = n.build_launch_command(&params);
        let js = j.build_launch_command(&params);
        // Same env preamble + same trailing claude invocation. The JS
        // form uses cross-platform host primitives so it should match
        // native byte-for-byte for the no-task case.
        assert_eq!(nat, js, "native vs js launch mismatch");
    }

    #[test]
    fn launch_command_headless_uses_stdin_pipe_in_both() {
        if !ws_exists() { return; }
        let (n, j) = pair();
        let params = AgentLaunchParams {
            task: Some("count files in repo".into()),
            skip_permissions: true,
            headless: true,
            ..Default::default()
        };
        let nat = n.build_launch_command(&params);
        let js = j.build_launch_command(&params);
        for cmd in [&nat, &js] {
            if cfg!(target_os = "windows") {
                assert!(cmd.contains("Get-Content "), "headless missing pipe: {cmd}");
                assert!(cmd.contains(" -Raw -Encoding UTF8 |"));
            } else {
                assert!(cmd.contains("cat "), "headless missing pipe: {cmd}");
                assert!(cmd.contains(" |"));
            }
            assert!(cmd.contains(" -p"));
            assert!(cmd.contains("--output-format stream-json"));
            assert!(cmd.contains("--verbose"));
            assert!(cmd.contains("--allowedTools Bash"));
            assert!(cmd.contains("--dangerously-skip-permissions"));
            assert!(cmd.contains(".ct-task-"));
        }
    }

    #[test]
    fn launch_command_headless_writes_task_bytes_through_plugin() {
        if !ws_exists() { return; }
        let (_n, j) = pair();
        let body = "do thing --kind fact `whoami` 'quotes'";
        let cmd = j.build_launch_command(&AgentLaunchParams {
            task: Some(body.into()),
            skip_permissions: true,
            headless: true,
            ..Default::default()
        });
        // Pull the task-file path out of the pipe prefix and verify the
        // bytes the plugin wrote round-trip byte-exact to the task body.
        let token = if cfg!(target_os = "windows") {
            cmd.split("Get-Content '").nth(1).and_then(|s| s.split('\'').next())
        } else {
            cmd.split("cat '").nth(1).and_then(|s| s.split('\'').next())
        };
        let path = token.expect("path token");
        let bytes = std::fs::read(path).expect("temp task file written");
        let written = if cfg!(target_os = "windows") {
            assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]), "BOM expected on Windows");
            String::from_utf8(bytes[3..].to_vec()).unwrap()
        } else {
            String::from_utf8(bytes).unwrap()
        };
        assert_eq!(written, body, "task body must round-trip byte-exact through plugin");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn launch_command_session_id_only_in_visible_mode() {
        if !ws_exists() { return; }
        let (n, j) = pair();
        let visible = AgentLaunchParams {
            session_id: Some("my-uuid".into()),
            skip_permissions: true,
            ..Default::default()
        };
        for cmd in [n.build_launch_command(&visible), j.build_launch_command(&visible)] {
            assert!(cmd.contains("--session-id"), "visible must inject: {cmd}");
            assert!(cmd.contains("my-uuid"));
        }
        let headless = AgentLaunchParams {
            task: Some("x".into()),
            session_id: Some("should-not-appear".into()),
            skip_permissions: true,
            headless: true,
            ..Default::default()
        };
        for cmd in [n.build_launch_command(&headless), j.build_launch_command(&headless)] {
            assert!(!cmd.contains("--session-id"), "headless must skip: {cmd}");
            assert!(!cmd.contains("should-not-appear"));
        }
    }

    #[test]
    fn session_jsonl_path_resolves_via_plugin() {
        if !ws_exists() { return; }
        let j = load_js("claude");
        let p = j.session_jsonl_path("/Users/foo/work/proj", "sess-abc-123");
        assert!(p.is_some(), "should return Some(path)");
        let p = p.unwrap();
        let s = p.to_string_lossy();
        assert!(s.contains("sess-abc-123.jsonl"), "missing session id: {s}");
        // Dash-encoded cwd — non-alphanumerics become '-'
        assert!(s.contains("-Users-foo-work-proj"), "dash-encoded cwd missing: {s}");
        assert!(s.contains(".claude") && s.contains("projects"),
            "must hit ~/.claude/projects: {s}");
    }

    #[test]
    fn session_jsonl_path_returns_none_for_empty_sid() {
        if !ws_exists() { return; }
        let j = load_js("claude");
        assert!(j.session_jsonl_path("/proj", "").is_none(),
            "empty session id must return None");
    }

    #[test]
    fn parse_session_delta_handles_polymorphic_content() {
        if !ws_exists() { return; }
        let j = load_js("claude");

        // Mixed JSONL: summary (drop), user text (1 block), assistant with
        // text + thinking + tool_use (3 blocks), user tool_result (1 block),
        // and a malformed line (drop).
        let jsonl = concat!(
            r#"{"type":"summary","x":1}"#, "\n",
            r#"{"type":"user","message":{"role":"user","content":"hello"},"timestamp":"2026-05-20T10:00:00Z"}"#, "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"hi"},{"type":"thinking","thinking":"hmm"},{"type":"tool_use","id":"u1","name":"Bash","input":{"command":"ls"}}]}}"#, "\n",
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"u1","content":"foo bar"}]}}"#, "\n",
            "not-json-at-all\n",
        );

        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), jsonl).unwrap();

        let delta = j.parse_session_delta(tmp.path(), 0);
        assert_eq!(delta.new_offset, jsonl.len() as u64,
            "should consume the entire file (last line ends in \\n)");
        assert_eq!(delta.messages.len(), 3, "3 user/assistant entries");

        let m0 = &delta.messages[0];
        assert_eq!(m0.role, "user");
        assert_eq!(m0.blocks.len(), 1);
        assert_eq!(m0.blocks[0].kind, "text");

        let m1 = &delta.messages[1];
        assert_eq!(m1.role, "assistant");
        assert_eq!(m1.blocks.len(), 3);
        let kinds: Vec<&str> = m1.blocks.iter().map(|b| b.kind.as_str()).collect();
        assert_eq!(kinds, vec!["text", "thinking", "tool_use"]);
        assert_eq!(
            m1.blocks[2].data.get("name").and_then(|v| v.as_str()),
            Some("Bash"),
        );

        let m2 = &delta.messages[2];
        assert_eq!(m2.blocks[0].kind, "tool_result");
        assert_eq!(
            m2.blocks[0].data.get("tool_use_id").and_then(|v| v.as_str()),
            Some("u1"),
        );
    }

    #[test]
    fn parse_session_delta_holds_back_partial_line() {
        if !ws_exists() { return; }
        let j = load_js("claude");

        // One complete line + a truncated second line.
        let jsonl = concat!(
            r#"{"type":"user","message":{"role":"user","content":"first"},"timestamp":"2026-05-20T10:00:00Z"}"#, "\n",
            r#"{"type":"user","message":{"role":"user","content":"sec"#,
        );
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), jsonl).unwrap();
        let delta = j.parse_session_delta(tmp.path(), 0);
        assert_eq!(delta.messages.len(), 1, "partial line must be held back");
        // new_offset = exactly the byte after the first \n
        let first_line_end = (jsonl.find('\n').unwrap() + 1) as u64;
        assert_eq!(delta.new_offset, first_line_end);
    }

    #[test]
    fn parse_session_delta_from_nonzero_offset() {
        if !ws_exists() { return; }
        let j = load_js("claude");
        // Two lines; we ask for the slice starting at line 2.
        let line1 = r#"{"type":"user","message":{"role":"user","content":"first"}}"#;
        let line2 = r#"{"type":"assistant","message":{"role":"assistant","content":"second"}}"#;
        let jsonl = format!("{}\n{}\n", line1, line2);
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &jsonl).unwrap();

        let offset = (line1.len() + 1) as u64;
        let delta = j.parse_session_delta(tmp.path(), offset);
        assert_eq!(delta.messages.len(), 1);
        assert_eq!(delta.messages[0].role, "assistant");
        assert_eq!(delta.new_offset, jsonl.len() as u64);
    }
}

// ────────────────────────────────────────────────────────────────────
// Codex parity
// ────────────────────────────────────────────────────────────────────

mod codex {
    use super::*;
    use crate::plugin::agent_codex::CodexProvider;

    fn pair() -> (CodexProvider, JsProvider) {
        (CodexProvider::new(), load_js("codex"))
    }

    #[test]
    fn detect_from_title_parity() {
        if !ws_exists() { return; }
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
        if !ws_exists() { return; }
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
        if !ws_exists() { return; }
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
        if !ws_exists() { return; }
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
        assert_eq!(n.has_event_markers("just normal output"), j.has_event_markers("just normal output"));
    }

    #[test]
    fn slash_commands_parity() {
        if !ws_exists() { return; }
        let (n, j) = pair();
        assert_eq!(n.compact_command(), j.compact_command());
        assert_eq!(n.exit_command(), j.exit_command());
        assert_eq!(n.auto_approve_flags(), j.auto_approve_flags());
        assert_eq!(n.model_launch_args("gpt-5.5"), j.model_launch_args("gpt-5.5"));
    }

    #[test]
    fn parse_session_messages_parity_on_rollout_fixture() {
        if !ws_exists() { return; }
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
        if !ws_exists() { return; }
        let (n, j) = pair();
        assert_eq!(n.id(), j.id());
        assert_eq!(n.display_name(), j.display_name());
        assert_eq!(n.binary_name(), j.binary_name());
        assert_eq!(n.cli_binary(), j.cli_binary());
        assert_eq!(n.icon_hint(), j.icon_hint());
        assert_eq!(n.icon_color(), j.icon_color());
        assert_eq!(n.status_bar_rows(), j.status_bar_rows());
        assert_eq!(n.preserves_context_on_resume(), j.preserves_context_on_resume());
    }

    #[test]
    fn resume_command_carries_session_id() {
        if !ws_exists() { return; }
        let (_n, j) = pair();
        let cmd = j.build_resume_command("uuid-1234", false).unwrap();
        assert_eq!(cmd, "codex resume 'uuid-1234'");
        let cmd2 = j.build_resume_command("uuid-1234", true).unwrap();
        assert_eq!(cmd2, "codex resume --dangerously-bypass-approvals-and-sandbox 'uuid-1234'");
    }

    /// Build a synthetic `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`
    /// tree in a tempdir, point HOME at it, then walk through the
    /// plugin. Verifies enumerateSessions, detectSessionId, and
    /// sessionFilePath all work end-to-end without native Rust help.
    #[test]
    fn codex_rollout_walk_via_plugin() {
        if !ws_exists() { return; }
        let dir = tempfile::tempdir().unwrap();
        // Plugin honours `CODETERM_CODEX_SESSIONS_ROOT` to avoid hitting
        // the user's real `~/.codex/sessions` during tests. (dirs::home_dir
        // ignores HOME env on Windows, so HOME redirect would be ineffective.)
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
            format!(r#"{{"type":"session_meta","payload":{{"id":"{uuid_a}","cwd":"/proj/foo"}}}}
{{"type":"response_item","payload":{{"type":"message","role":"user","content":[]}}}}
"#),
        )
        .unwrap();
        std::fs::write(
            &file_b,
            format!(r#"{{"type":"session_meta","payload":{{"id":"{uuid_b}","cwd":"/other/dir"}}}}
"#),
        )
        .unwrap();
        std::fs::write(
            &file_c,
            format!(r#"{{"type":"session_meta","payload":{{"id":"{uuid_c}","cwd":"/proj/foo"}}}}
"#),
        )
        .unwrap();
        // Touch modified times so c > a > b ordering is deterministic.
        let now = std::time::SystemTime::now();
        let _ = filetime::set_file_mtime(
            &file_a,
            filetime::FileTime::from_system_time(now - std::time::Duration::from_secs(60)),
        );
        let _ = filetime::set_file_mtime(
            &file_c,
            filetime::FileTime::from_system_time(now),
        );

        // Synthetic plugin pointing at our tempdir via HOME env.
        let plugin_js = std::fs::read_to_string(
            workspace_plugins_root().join("provider-codex").join("plugin.js"),
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
        let by_id: std::collections::HashMap<_, _> =
            all.iter().map(|s| (s.session_id.clone(), s.clone())).collect();
        assert_eq!(by_id.len(), 3, "expected 3 sessions, got {}: {by_id:#?}", by_id.len());
        assert_eq!(by_id[uuid_a].provider, "codex");
        assert_eq!(by_id[uuid_a].project_path.as_deref(), Some("/proj/foo"));
        assert_eq!(by_id[uuid_b].project_path.as_deref(), Some("/other/dir"));

        // detectSessionId filters by cwd and returns the newest match.
        let latest = provider.detect_session_id("/proj/foo", &[], None);
        assert_eq!(latest.as_deref(), Some(uuid_c));

        let latest_excl = provider.detect_session_id("/proj/foo", &[uuid_c.to_string()], None);
        assert_eq!(latest_excl.as_deref(), Some(uuid_a));

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
}

// ────────────────────────────────────────────────────────────────────
// Opencode parity
// ────────────────────────────────────────────────────────────────────

mod opencode {
    use super::*;
    use crate::plugin::agent_opencode::OpenCodeProvider;

    fn pair() -> (OpenCodeProvider, JsProvider) {
        (OpenCodeProvider::new(), load_js("opencode"))
    }

    #[test]
    fn detect_from_title_parity() {
        if !ws_exists() { return; }
        let (n, j) = pair();
        for s in ["OpenCode", "opencode v0.3", "OpenCode Zen"] {
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
        if !ws_exists() { return; }
        let (n, j) = pair();
        // Native + JS both require a two-part fingerprint: marker
        // (opencode/OpenCode) AND a TUI/prompt signal (box-drawing or a
        // permission line). Bare mentions or bare signals must NOT match.
        let cases = [
            ("opencode\nPermission required for tool call", true),
            ("OpenCode Zen ┃ banner", true),
            ("OpenCode running\nBuild · 2025-01-01", true),
            ("opencode session", false),
            ("Permission required for tool call", false),
            ("OpenCode Zen banner", false),
            ("just shell output", false),
        ];
        for (s, expect) in cases {
            assert_eq!(n.detect_from_output(s), expect, "native: {s}");
            assert_eq!(j.detect_from_output(s), expect, "js: {s}");
        }
    }

    #[test]
    fn screen_has_tui_parity() {
        if !ws_exists() { return; }
        let (n, j) = pair();
        for s in ["tab agents", "Ctrl+P commands"] {
            assert!(n.screen_has_tui(s));
            assert!(j.screen_has_tui(s));
        }
        for s in ["plain shell"] {
            assert!(!n.screen_has_tui(s));
            assert!(!j.screen_has_tui(s));
        }
    }

    #[test]
    fn slash_commands_and_flags_parity() {
        if !ws_exists() { return; }
        let (n, j) = pair();
        assert_eq!(n.compact_command(), j.compact_command());
        assert_eq!(n.exit_command(), j.exit_command());
        assert_eq!(n.auto_approve_flags(), j.auto_approve_flags());
        assert_eq!(
            n.model_launch_args("anthropic/claude-opus-4-7"),
            j.model_launch_args("anthropic/claude-opus-4-7"),
        );
    }

    #[test]
    fn metadata_parity() {
        if !ws_exists() { return; }
        let (n, j) = pair();
        assert_eq!(n.id(), j.id());
        assert_eq!(n.display_name(), j.display_name());
        assert_eq!(n.binary_name(), j.binary_name());
        assert_eq!(n.cli_binary(), j.cli_binary());
        assert_eq!(n.icon_hint(), j.icon_hint());
        assert_eq!(n.icon_color(), j.icon_color());
        assert_eq!(n.status_bar_rows(), j.status_bar_rows());
    }

    #[test]
    fn resume_command_drops_skip_flag_by_design() {
        if !ws_exists() { return; }
        let (_n, j) = pair();
        let cmd = j.build_resume_command("ses_abc123", true).unwrap();
        assert!(!cmd.contains("--dangerously-skip-permissions"));
        assert!(cmd.contains("--session"));
        assert!(cmd.contains("ses_abc123"));
    }

    /// Stand up a synthetic opencode.db at $XDG_DATA_HOME/opencode/opencode.db,
    /// seed it with rows that mirror the real schema, then point the plugin
    /// at it via env override and exercise detectSessionId / detectActiveSession
    /// / detectSessionModel / sessionExists end-to-end through `JsProvider`.
    #[test]
    fn opencode_sqlite_session_detection_via_plugin() {
        if !ws_exists() { return; }
        let dir = tempfile::tempdir().unwrap();
        let opencode_dir = dir.path().join("opencode");
        std::fs::create_dir_all(&opencode_dir).unwrap();
        let db_path = opencode_dir.join("opencode.db");

        // Seed a sqlite db with the columns our plugin queries.
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE session (id TEXT PRIMARY KEY, directory TEXT, time_created INTEGER, time_archived INTEGER);\
                 CREATE TABLE message (session_id TEXT, time_created INTEGER, data TEXT);",
            )
            .unwrap();
            let cwd_fwd = "/proj/foo";
            let cwd_bwd = "\\proj\\foo";
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;
            // 3 live sessions for our cwd at decreasing time_created;
            // 1 archived; 1 in another dir; 1 explicitly old.
            conn.execute(
                "INSERT INTO session VALUES (?1, ?2, ?3, NULL)",
                rusqlite::params!["ses_new", cwd_fwd, now - 1000],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO session VALUES (?1, ?2, ?3, NULL)",
                rusqlite::params!["ses_mid", cwd_bwd, now - 10_000],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO session VALUES (?1, ?2, ?3, NULL)",
                rusqlite::params!["ses_excluded", cwd_fwd, now - 2_000],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO session VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params!["ses_arch", cwd_fwd, now - 500, now],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO session VALUES (?1, ?2, ?3, NULL)",
                rusqlite::params!["ses_other_cwd", "/other/place", now - 200],
            )
            .unwrap();
            // Stale session > maxAge.
            conn.execute(
                "INSERT INTO session VALUES (?1, ?2, ?3, NULL)",
                rusqlite::params!["ses_stale", cwd_fwd, now - 10_000_000],
            )
            .unwrap();
            // Message rows for detectSessionModel / detectActiveSession.
            conn.execute(
                "INSERT INTO message VALUES (?1, ?2, ?3)",
                rusqlite::params!["ses_new", now - 800,
                    r#"{"role":"assistant","modelID":"anthropic/claude-opus-4-7"}"#],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO message VALUES (?1, ?2, ?3)",
                rusqlite::params!["ses_mid", now - 5_000,
                    r#"{"role":"assistant","modelID":"anthropic/claude-sonnet-4-6"}"#],
            )
            .unwrap();
        }

        // Load opencode plugin with XDG_DATA_HOME pointed at our tempdir.
        let plugin_js = std::fs::read_to_string(
            workspace_plugins_root().join("provider-opencode").join("plugin.js"),
        )
        .unwrap();
        let manifest_json = r#"{
            "id":"opencode-test","version":"0","displayName":"X","binaryName":"opencode",
            "spawn":{"readinessStrategy":{"kind":"fixed_delay","durationMs":0},
                     "taskDelivery":"pty_input","verifySubmit":{"kind":"none"}}
        }"#;
        let manifest: crate::plugin::js_runtime::PluginManifest =
            serde_json::from_str(manifest_json).unwrap();
        // SAFETY: integration tests; harmless env mutation scoped to this fn.
        unsafe {
            std::env::set_var("XDG_DATA_HOME", dir.path().to_string_lossy().as_ref());
        }
        let vm = crate::plugin::js_runtime::vm::PluginVm::from_source_with(
            "opencode-test",
            plugin_js,
            Some(manifest_json.to_string()),
        )
        .unwrap();
        let provider = JsProvider::new(manifest, std::sync::Arc::new(vm));

        // Newest live session in /proj/foo with no exclusion → ses_new.
        let sid = provider.detect_session_id("/proj/foo", &[], None);
        assert_eq!(sid.as_deref(), Some("ses_new"));

        // Exclusion list skips ses_new → next newest is ses_excluded.
        let sid2 = provider.detect_session_id(
            "/proj/foo",
            &["ses_new".to_string()],
            None,
        );
        assert_eq!(sid2.as_deref(), Some("ses_excluded"));

        // Strict max_age = 50 ms — even ses_new (now-1000) misses.
        let none = provider.detect_session_id(
            "/proj/foo",
            &[],
            Some(std::time::Duration::from_millis(50)),
        );
        assert!(none.is_none(), "max_age filter must drop everything");

        // Archived + other-cwd rows must NOT surface.
        let sid_other = provider.detect_session_id("/totally/different/dir", &[], None);
        assert_eq!(sid_other, None);

        // sessionExists hits the live row.
        assert!(provider.session_exists("/proj/foo", "ses_new"));
        assert!(!provider.session_exists("/proj/foo", "ses_does_not_exist"));

        // detectSessionModel reads from the assistant row for the session.
        let model = provider.detect_session_model("/proj/foo", "ses_new");
        assert_eq!(model.as_deref(), Some("anthropic/claude-opus-4-7"));

        // detectActiveSession returns the session id with the most recent
        // message in the cwd.
        let active = provider.detect_active_session("/proj/foo", "");
        assert_eq!(active.as_deref(), Some("ses_new"));

        // Cleanup env so other tests aren't affected.
        unsafe { std::env::remove_var("XDG_DATA_HOME"); }
    }
}

// ────────────────────────────────────────────────────────────────────
// Loader behaviour
// ────────────────────────────────────────────────────────────────────

#[test]
fn all_workspace_plugins_load() {
    if !ws_exists() { return; }
    for id in ["claude", "codex", "opencode"] {
        let p = load_js(id);
        assert_eq!(p.id(), id, "plugin id mismatch for {id}");
    }
}
