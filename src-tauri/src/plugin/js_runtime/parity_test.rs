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

type PluginsDir = PathBuf;

fn workspace_plugins_root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/..")).join("plugins")
}

fn require_plugins() -> Option<PluginsDir> {
    let path = workspace_plugins_root();
    if path.exists() {
        return Some(path);
    }

    if std::env::var("CI").is_ok() {
        panic!("parity tests require bundled JS plugins in CI — build them in the workflow or fix the path");
    }

    eprintln!(
        "SKIPPED: js_runtime parity — plugins dir not found at {}",
        path.display()
    );
    None
}

fn load_js(id: &str) -> JsProvider {
    let manifest = workspace_plugins_root()
        .join(format!("provider-{id}"))
        .join("plugin.json");
    load_one(&manifest).unwrap_or_else(|e| panic!("load {id} plugin: {e}"))
}

// Claude parity.

mod claude {
    use super::*;
    use crate::plugin::agent_claude::provider::ClaudeProvider;

    fn pair() -> (ClaudeProvider, JsProvider) {
        (ClaudeProvider::new(), load_js("claude"))
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
        assert_eq!(
            n.preserves_context_on_resume(),
            j.preserves_context_on_resume()
        );
        assert_eq!(n.has_hooks(), j.has_hooks());
        assert_eq!(n.supports_lazy_skills(), j.supports_lazy_skills());
        assert_eq!(n.status_bar_rows(), j.status_bar_rows());
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
    fn resume_command_carries_session_id_and_skip_flag() {
        if require_plugins().is_none() {
            return;
        }
        let (_n, j) = pair();
        let cmd = j.build_resume_command("abc-def-123-456", true).unwrap();
        assert!(cmd.contains("--resume"));
        assert!(cmd.contains("abc-def-123-456"));
        assert!(cmd.contains("--dangerously-skip-permissions"));
    }

    /// Synthetic Claude plugin variant that points its rule + integration
    /// paths into a tempdir so we exercise the real plugin.js end-to-end
    /// against `JsProvider` without touching the user's real `~/.claude/`.
    fn claude_in_temp(rule_path: &std::path::Path, settings_path: &std::path::Path) -> JsProvider {
        let plugin_js = std::fs::read_to_string(
            workspace_plugins_root()
                .join("provider-claude")
                .join("plugin.js"),
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
        provider.install_integration(7686).expect("install ok");
        let json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("settings.json")).unwrap(),
        )
        .unwrap();
        for key in ["Notification", "Stop", "PreCompact", "SessionStart"] {
            let arr = json["hooks"][key].as_array().expect(key);
            assert!(
                arr.iter().any(|e| {
                    e["hooks"].as_array().map_or(false, |inner| {
                        inner.iter().any(|h| h["_source"] == "codeterm-hook-v2")
                    })
                }),
                "missing v2 hook in {key}"
            );
            // Curl points to the right port + path.
            let any_cmd = arr
                .iter()
                .flat_map(|e| e["hooks"].as_array().cloned().unwrap_or_default())
                .find_map(|h| h["command"].as_str().map(String::from))
                .unwrap();
            assert!(
                any_cmd.contains("CODETERM_API_PORT:-7686"),
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
                e["hooks"][0]["_source"]
                    .as_str()
                    .map_or(true, |s| s != "codeterm")
            }),
            "v1 stale entry must be dropped"
        );
        assert!(arr
            .iter()
            .any(|e| { e["hooks"][0]["_source"] == "codeterm-hook-v2" }));
    }

    #[test]
    fn integration_uninstall_drops_notification_and_stop_keeps_precompact() {
        if require_plugins().is_none() {
            return;
        }
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
        // Mirroring native: Notification + Stop dropped; PreCompact + SessionStart kept (mem continuity).
        let n = json["hooks"]["Notification"].as_array().unwrap();
        let s = json["hooks"]["Stop"].as_array().unwrap();
        let pc = json["hooks"]["PreCompact"].as_array().unwrap();
        let ss = json["hooks"]["SessionStart"].as_array().unwrap();
        assert!(n
            .iter()
            .all(|e| e["hooks"][0]["_source"] != "codeterm-hook-v2"));
        assert!(s
            .iter()
            .all(|e| e["hooks"][0]["_source"] != "codeterm-hook-v2"));
        assert!(pc
            .iter()
            .any(|e| e["hooks"][0]["_source"] == "codeterm-hook-v2"));
        assert!(ss
            .iter()
            .any(|e| e["hooks"][0]["_source"] == "codeterm-hook-v2"));
    }

    #[test]
    fn integration_ensure_on_startup_is_idempotent() {
        if require_plugins().is_none() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let provider = claude_in_temp(
            &dir.path().join("rule.md"),
            &dir.path().join("settings.json"),
        );
        provider.ensure_integration_on_startup(7686);
        let first = std::fs::read_to_string(dir.path().join("settings.json")).unwrap();
        provider.ensure_integration_on_startup(7686);
        let second = std::fs::read_to_string(dir.path().join("settings.json")).unwrap();
        // Second pass must NOT duplicate hooks — checkIntegration sees installed=true and skips re-write.
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
    fn launch_command_headless_uses_stdin_pipe_in_both() {
        if require_plugins().is_none() {
            return;
        }
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
        if require_plugins().is_none() {
            return;
        }
        let (_n, j) = pair();
        let body = "do thing --kind fact `whoami` 'quotes'";
        let cmd = j.build_launch_command(&AgentLaunchParams {
            task: Some(body.into()),
            skip_permissions: true,
            headless: true,
            ..Default::default()
        });
        // Pull the task-file path out of the pipe prefix and verify the bytes the plugin wrote round-trip byte-exact to the task body.
        let token = if cfg!(target_os = "windows") {
            cmd.split("Get-Content '")
                .nth(1)
                .and_then(|s| s.split('\'').next())
        } else {
            cmd.split("cat '").nth(1).and_then(|s| s.split('\'').next())
        };
        let path = token.expect("path token");
        let bytes = std::fs::read(path).expect("temp task file written");
        let written = if cfg!(target_os = "windows") {
            assert!(
                bytes.starts_with(&[0xEF, 0xBB, 0xBF]),
                "BOM expected on Windows"
            );
            String::from_utf8(bytes[3..].to_vec()).unwrap()
        } else {
            String::from_utf8(bytes).unwrap()
        };
        assert_eq!(
            written, body,
            "task body must round-trip byte-exact through plugin"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn launch_command_session_id_only_in_visible_mode() {
        if require_plugins().is_none() {
            return;
        }
        let (n, j) = pair();
        let visible = AgentLaunchParams {
            session_id: Some("my-uuid".into()),
            skip_permissions: true,
            ..Default::default()
        };
        for cmd in [
            n.build_launch_command(&visible),
            j.build_launch_command(&visible),
        ] {
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
        for cmd in [
            n.build_launch_command(&headless),
            j.build_launch_command(&headless),
        ] {
            assert!(!cmd.contains("--session-id"), "headless must skip: {cmd}");
            assert!(!cmd.contains("should-not-appear"));
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
        // Must encode the canonical (real) target, not the symlink alias —
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

    /// A JS provider that reimplements AskUserQuestion extraction in JS,
    /// declaring `interactiveEvents: true` so the host dispatches to it.
    fn claude_interactive_js() -> JsProvider {
        let manifest_json = r#"{
            "id":"claude-interactive","version":"0.1.0","displayName":"X",
            "spawn":{"readinessStrategy":{"kind":"fixed_delay","durationMs":0},
                     "taskDelivery":"pty_input",
                     "verifySubmit":{"kind":"none"},
                     "interactiveEvents":true}
        }"#;
        let plugin_js = r#"
            module.exports.default = {
                parseInteractiveEvents(msg) {
                    var out = [];
                    var blocks = (msg && msg.blocks) || [];
                    for (var bi = 0; bi < blocks.length; bi++) {
                        var b = blocks[bi];
                        if (!b || b.kind !== "tool_use") continue;
                        var data = b.data || {};
                        if (data.name !== "AskUserQuestion") continue;
                        var toolId = data.id || "";
                        var input = data.input || {};
                        var questions = input.questions;
                        if (!Array.isArray(questions)) continue;
                        for (var i = 0; i < questions.length; i++) {
                            var q = questions[i];
                            if (!q || !Array.isArray(q.options)) continue;
                            var options = [];
                            for (var oi = 0; oi < q.options.length; oi++) {
                                var o = q.options[oi];
                                if (!o || typeof o.label !== "string") continue;
                                var opt = { label: o.label };
                                if (typeof o.description === "string") opt.description = o.description;
                                options.push(opt);
                            }
                            var elem = {
                                type: "question",
                                id: toolId + "-" + i,
                                toolUseId: toolId,
                                prompt: typeof q.question === "string" ? q.question : "",
                                options: options,
                                multiSelect: q.multiSelect === true
                            };
                            if (typeof q.header === "string") elem.header = q.header;
                            out.push(elem);
                        }
                    }
                    return out;
                }
            };
        "#;
        let manifest: crate::plugin::js_runtime::PluginManifest =
            serde_json::from_str(manifest_json).unwrap();
        let vm = crate::plugin::js_runtime::vm::PluginVm::from_source_with(
            "claude-interactive",
            plugin_js.to_string(),
            Some(manifest_json.to_string()),
        )
        .unwrap();
        JsProvider::new(manifest, std::sync::Arc::new(vm))
    }

    fn ask_user_question_fixture() -> crate::agent::chat_types::RichMessage {
        use crate::agent::chat_types::{MessageBlock, RichMessage};
        use serde_json::json;
        RichMessage {
            role: "assistant".into(),
            blocks: vec![
                MessageBlock {
                    kind: "text".into(),
                    data: json!({"text": "deciding"}),
                },
                MessageBlock {
                    kind: "tool_use".into(),
                    data: json!({
                        "id": "toolu_parity",
                        "name": "AskUserQuestion",
                        "input": {
                            "questions": [
                                {
                                    "question": "Pick a runtime",
                                    "header": "Runtime",
                                    "multiSelect": false,
                                    "options": [
                                        {"label": "Node", "description": "JS runtime"},
                                        {"label": "Deno"}
                                    ]
                                },
                                {
                                    "question": "Pick features",
                                    "multiSelect": true,
                                    "options": [
                                        {"label": "TLS"},
                                        {"label": "HTTP2", "description": "multiplexed"}
                                    ]
                                }
                            ]
                        }
                    }),
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn parse_interactive_events_native_js_parity() {
        // No bundled-plugin dependency (inline JS + native impl), so this always
        // runs — the parity gate must not silently skip.
        let n = ClaudeProvider::new();
        let j = claude_interactive_js();
        let fixture = ask_user_question_fixture();
        let native = n.parse_interactive_events(&fixture);
        let js = j.parse_interactive_events(&fixture);
        assert_eq!(native.len(), 2, "native must extract two questions");
        assert_eq!(
            native, js,
            "native and JS parseInteractiveEvents must agree on the same fixture"
        );
    }

    #[test]
    fn parse_session_delta_handles_polymorphic_content() {
        if require_plugins().is_none() {
            return;
        }
        let j = load_js("claude");

        // Mixed JSONL: summary (drop), user text (1 block), assistant with text + thinking + tool_use (3 blocks), user tool_result (1 block), and a malformed line (drop).
        let jsonl = concat!(
            r#"{"type":"summary","x":1}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":"hello"},"timestamp":"2026-05-20T10:00:00Z"}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"hi"},{"type":"thinking","thinking":"hmm"},{"type":"tool_use","id":"u1","name":"Bash","input":{"command":"ls"}}]}}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"u1","content":"foo bar"}]}}"#,
            "\n",
            "not-json-at-all\n",
        );

        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), jsonl).unwrap();

        let delta = j.parse_session_delta(tmp.path(), 0);
        assert_eq!(
            delta.new_offset,
            jsonl.len() as u64,
            "should consume the entire file (last line ends in \\n)"
        );
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
            m2.blocks[0]
                .data
                .get("tool_use_id")
                .and_then(|v| v.as_str()),
            Some("u1"),
        );
    }

    /// Eval: the host flatten path (real plugin.js `parseSessionDelta` →
    /// Rust `rich_to_chat_msgs`) must produce the SAME normalized `ChatMsg`
    /// shape the desktop TS parser (`parseJsonlSession`) produces, so the
    /// desktop can switch to the host path and the TS duplicate can be
    /// deleted. This is the bench guarding that consolidation.
    #[test]
    fn rich_flatten_parity_block_derivable_kinds() {
        if require_plugins().is_none() {
            return;
        }
        use crate::service::pane_chat::rich_to_chat_msgs;
        let j = load_js("claude");

        // Covers every block kind plugin.js already passes through:
        // user text, assistant text, thinking, tool_use, assistant-turn
        // tool_result, user-turn tool_result, user image attachment,
        // assistant standalone image.
        let jsonl = concat!(
            r#"{"type":"user","message":{"role":"user","content":"refactor please"},"timestamp":"2026-05-20T10:00:00Z"}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"on it"},{"type":"thinking","thinking":"let me think a lot AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA done"},{"type":"tool_use","id":"u1","name":"Bash","input":{"command":"ls -la /very/long/path/that/exceeds/eighty/characters/for/sure/abcdefghijklmnop"}}]}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_result","tool_use_id":"u1","content":"file listing"}]}}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"u2","content":"grep output"}]}}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"see image"},{"type":"image","source":{"media_type":"image/png","data":"BASE64DATA"}}]}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"image","source":{"media_type":"image/jpeg","data":"IMGBYTES"}}]}}"#,
            "\n",
        );
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), jsonl).unwrap();

        let delta = j.parse_session_delta(tmp.path(), 0);
        let msgs: Vec<_> = delta
            .messages
            .iter()
            .enumerate()
            .flat_map(|(i, r)| rich_to_chat_msgs(r, i))
            .collect();

        let by_kind = |k: &str| msgs.iter().filter(|m| m.kind == k).collect::<Vec<_>>();

        // user text
        let users = by_kind("user");
        assert!(
            users.iter().any(|m| m.content == "refactor please"),
            "user text lost: {users:#?}"
        );

        // assistant text
        assert!(
            by_kind("assistant").iter().any(|m| m.content == "on it"),
            "assistant text lost"
        );

        // thinking — truncated to 200 chars + ellipsis, collapsed
        let think = by_kind("thinking");
        assert_eq!(think.len(), 1, "exactly one thinking block");
        assert!(think[0].collapsed, "thinking must be collapsed");
        assert!(
            think[0].content.starts_with("let me think a lot"),
            "thinking content lost: {:?}",
            think[0].content
        );
        assert!(
            think[0].content.ends_with("...") && think[0].content.len() <= 205,
            "thinking must be truncated with ellipsis: len={}",
            think[0].content.len()
        );

        // tool_call — toolName, toolId, collapsed, toolInput preserved, toolArgs summarized (command, ≤80)
        let calls = by_kind("tool_call");
        assert_eq!(calls.len(), 1, "one tool_call");
        let call = calls[0];
        assert_eq!(call.tool_name.as_deref(), Some("Bash"));
        assert_eq!(call.tool_id.as_deref(), Some("u1"));
        assert!(call.collapsed, "tool_call collapsed");
        assert!(
            call.tool_input.is_some(),
            "raw tool_input must survive for diff rendering"
        );
        let args = call.tool_args.as_deref().unwrap_or("");
        assert!(args.starts_with("ls -la"), "toolArgs summary: {args}");
        assert!(
            args.len() <= 80,
            "toolArgs must be capped at 80: {}",
            args.len()
        );

        // tool_result — both assistant-turn (u1) and user-turn (u2), toolId + collapsed
        let results = by_kind("tool_result");
        assert_eq!(results.len(), 2, "both tool_results surfaced: {results:#?}");
        assert!(results.iter().any(|m| m.tool_id.as_deref() == Some("u1")
            && m.tool_result.as_deref() == Some("file listing")));
        assert!(results.iter().any(|m| m.tool_id.as_deref() == Some("u2")
            && m.tool_result.as_deref() == Some("grep output")));
        assert!(results.iter().all(|m| m.collapsed));

        // user image attachment — on the user message carrying text "see image"
        let with_attach = users
            .iter()
            .find(|m| m.attachments.is_some())
            .expect("user image attachment lost");
        let atts = with_attach.attachments.as_ref().unwrap();
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0].mime_type, "image/png");
        assert_eq!(atts[0].data.as_deref(), Some("BASE64DATA"));

        // assistant standalone image
        let images = by_kind("image");
        assert_eq!(images.len(), 1, "standalone image lost");
        assert_eq!(images[0].image_mime_type.as_deref(), Some("image/jpeg"));
        assert_eq!(images[0].image_data.as_deref(), Some("IMGBYTES"));
    }

    /// Eval: system entries, isMeta rows, and local-command echoes must
    /// normalize to the same system / local_command shapes the desktop
    /// parser produced — these need plugin.js to carry uuid/isMeta and emit
    /// `system` rows, so this guards that plumbing end-to-end.
    #[test]
    fn rich_flatten_parity_system_meta_local_command() {
        if require_plugins().is_none() {
            return;
        }
        use crate::service::pane_chat::rich_to_chat_msgs;
        let j = load_js("claude");

        let jsonl = concat!(
            // plain system row with a subtype
            r#"{"type":"system","subtype":"hint","content":"heads up","uuid":"s1"}"#,
            "\n",
            // isMeta user row → system, skill_context subtype via content prefix
            r#"{"type":"user","isMeta":true,"uuid":"m1","message":{"role":"user","content":"Base directory for this skill: /x/y"}}"#,
            "\n",
            // isMeta user row → system, generic meta subtype
            r#"{"type":"user","isMeta":true,"uuid":"m2","message":{"role":"user","content":"<system reminder>be nice"}}"#,
            "\n",
            // local-command echo in a user string turn → local_command + tagName
            r#"{"type":"user","uuid":"c1","message":{"role":"user","content":"<command-name>compact</command-name>"}}"#,
            "\n",
            // a normal user turn must still be a user message, with its uuid as id base
            r#"{"type":"user","uuid":"u9","message":{"role":"user","content":"hi there"}}"#,
            "\n",
        );
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), jsonl).unwrap();

        let delta = j.parse_session_delta(tmp.path(), 0);
        let msgs: Vec<_> = delta
            .messages
            .iter()
            .enumerate()
            .flat_map(|(i, r)| rich_to_chat_msgs(r, i))
            .collect();

        let find = |pred: &dyn Fn(&crate::agent::chat_types::ChatMsg) -> bool| {
            msgs.iter().find(|m| pred(m)).cloned()
        };

        let sys = find(&|m| m.kind == "system" && m.content == "heads up")
            .expect("plain system row lost — plugin.js must emit `system` entries");
        assert_eq!(sys.system_subtype.as_deref(), Some("hint"));

        let skill =
            find(&|m| m.kind == "system" && m.system_subtype.as_deref() == Some("skill_context"))
                .expect("isMeta skill_context row lost");
        assert!(skill.content.contains("Base directory"));

        let meta = find(&|m| m.kind == "system" && m.system_subtype.as_deref() == Some("meta"))
            .expect("isMeta meta row lost");
        assert!(meta.content.contains("be nice"));

        let local = find(&|m| m.kind == "local_command").expect("local-command echo lost");
        assert_eq!(local.tag_name.as_deref(), Some("command-name"));
        assert_eq!(local.content, "compact");

        // Normal user turn survives, and its id is uuid-derived (stable for
        // streaming mergeById, not a volatile delta index).
        let user =
            find(&|m| m.kind == "user" && m.content == "hi there").expect("normal user turn lost");
        assert!(
            user.id.starts_with("u9-"),
            "id must be uuid-based: {}",
            user.id
        );
    }

    #[test]
    fn parse_session_delta_holds_back_partial_line() {
        if require_plugins().is_none() {
            return;
        }
        let j = load_js("claude");

        // One complete line + a truncated second line.
        let jsonl = concat!(
            r#"{"type":"user","message":{"role":"user","content":"first"},"timestamp":"2026-05-20T10:00:00Z"}"#,
            "\n",
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
        if require_plugins().is_none() {
            return;
        }
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

    #[test]
    fn parse_session_delta_handles_non_ascii_offset_correctly() {
        if require_plugins().is_none() {
            return;
        }
        let j = load_js("claude");
        // Each line ends in \n. Contains emoji (4 bytes UTF-8 each) so byte counts diverge from char counts.
        let line1 =
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hello 🚀 мир 你好\"}}";
        let line2 = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"reply 🎉\"}}";
        let jsonl = format!("{}\n{}\n", line1, line2);
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &jsonl).unwrap();
        // First read consumes the whole file. new_offset must equal the BYTE length of jsonl, not a code-unit count.
        let delta = j.parse_session_delta(tmp.path(), 0);
        assert_eq!(
            delta.new_offset,
            jsonl.as_bytes().len() as u64,
            "new_offset must be byte-accurate even with non-ASCII content"
        );
        assert_eq!(delta.messages.len(), 2);
        // Second read from the previous new_offset must return nothing.
        let delta2 = j.parse_session_delta(tmp.path(), delta.new_offset);
        assert_eq!(delta2.messages.len(), 0);
        assert_eq!(delta2.new_offset, jsonl.as_bytes().len() as u64);
    }

    #[test]
    fn parse_session_delta_advances_offset_when_plugin_blows_up() {
        // Defensive: feed the plugin malformed JSONL bytes. Plugin catch-blocks each line, but the test asserts the tailer doesn't get stuck at the same offset.
        if require_plugins().is_none() {
            return;
        }
        let j = load_js("claude");
        let bad = "this is not json at all\nnot json either\n";
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), bad).unwrap();
        let delta = j.parse_session_delta(tmp.path(), 0);
        // No valid messages — but offset must advance to file end so we don't busy-loop on the same bytes.
        assert_eq!(delta.messages.len(), 0);
        assert_eq!(delta.new_offset, bad.as_bytes().len() as u64);
    }
}

// Codex parity.

mod codex;
// Opencode parity.

mod opencode {
    use super::*;
    use crate::plugin::agent_opencode::OpenCodeProvider;

    fn pair() -> (OpenCodeProvider, JsProvider) {
        (OpenCodeProvider::new(), load_js("opencode"))
    }

    #[test]
    fn detect_from_title_parity() {
        if require_plugins().is_none() {
            return;
        }
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
        if require_plugins().is_none() {
            return;
        }
        let (n, j) = pair();
        // Native + JS both require a two-part fingerprint: marker (opencode/OpenCode) AND a TUI/prompt signal (box-drawing or a permission line). Bare
        // mentions or bare signals must NOT match.
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
        if require_plugins().is_none() {
            return;
        }
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
        if require_plugins().is_none() {
            return;
        }
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
    }

    #[test]
    fn resume_command_drops_skip_flag_by_design() {
        if require_plugins().is_none() {
            return;
        }
        let (_n, j) = pair();
        let cmd = j.build_resume_command("ses_abc123", true).unwrap();
        assert!(!cmd.contains("--dangerously-skip-permissions"));
        assert!(cmd.contains("--session"));
        assert!(cmd.contains("ses_abc123"));
    }

    /// Stand up a synthetic opencode.db at $XDG_DATA_HOME/opencode/opencode.db,
    /// seed it with rows that mirror the real schema, then point the plugin
    /// at it via env override and exercise the exact-session sqlite operations
    /// through `JsProvider`. Hook-backed providers must not claim "newest for
    /// cwd"; binding comes from the provider-reported session id.
    #[test]
    fn opencode_sqlite_session_detection_via_plugin() {
        if require_plugins().is_none() {
            return;
        }
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
            // 3 live sessions for our cwd at decreasing time_created; 1 archived; 1 in another dir; 1 explicitly old.
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
                rusqlite::params![
                    "ses_new",
                    now - 800,
                    r#"{"role":"assistant","modelID":"anthropic/claude-opus-4-7"}"#
                ],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO message VALUES (?1, ?2, ?3)",
                rusqlite::params![
                    "ses_mid",
                    now - 5_000,
                    r#"{"role":"assistant","modelID":"anthropic/claude-sonnet-4-6"}"#
                ],
            )
            .unwrap();
        }

        // Load opencode plugin with XDG_DATA_HOME pointed at our tempdir.
        let plugin_js = std::fs::read_to_string(
            workspace_plugins_root()
                .join("provider-opencode")
                .join("plugin.js"),
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

        // Hook-backed provider: sqlite is not an ownership oracle.
        let sid = provider.detect_session_id("/proj/foo", &[], None);
        assert_eq!(sid, None);

        // Exclusions do not matter because no cwd-based claim is allowed.
        let sid2 = provider.detect_session_id("/proj/foo", &["ses_new".to_string()], None);
        assert_eq!(sid2, None);

        // max_age is ignored for ownership because the provider hook is required.
        let none = provider.detect_session_id(
            "/proj/foo",
            &[],
            Some(std::time::Duration::from_millis(50)),
        );
        assert!(
            none.is_none(),
            "cwd-based sqlite detection must stay disabled"
        );

        // Archived + other-cwd rows must NOT surface.
        let sid_other = provider.detect_session_id("/totally/different/dir", &[], None);
        assert_eq!(sid_other, None);

        // sessionExists hits the live row.
        assert!(provider.session_exists("/proj/foo", "ses_new"));
        assert!(!provider.session_exists("/proj/foo", "ses_does_not_exist"));

        // detectSessionModel reads from the assistant row for the session.
        let model = provider.detect_session_model("/proj/foo", "ses_new");
        assert_eq!(model.as_deref(), Some("anthropic/claude-opus-4-7"));

        // detectActiveSession also must not pick a different newest session.
        let active = provider.detect_active_session("/proj/foo", "");
        assert_eq!(active, None);

        // Cleanup env so other tests aren't affected.
        unsafe {
            std::env::remove_var("XDG_DATA_HOME");
        }
    }
}

// Loader behaviour.

#[test]
fn third_party_without_host_api_is_rejected_before_vm_load() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("unknown-third-party");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("plugin.json"),
        r#"{
            "id": "unknown-third-party",
            "version": "0.1.0",
            "displayName": "Unknown Third Party",
            "spawn": {
                "readinessStrategy": { "kind": "fixed_delay", "durationMs": 0 },
                "taskDelivery": "pty_input",
                "verifySubmit": { "kind": "none" }
            },
            "entry": "plugin.js"
        }"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("plugin.js"),
        "throw new Error('must not execute before hostApi validation');",
    )
    .unwrap();

    let err = match load_one(&dir.join("plugin.json")) {
        Ok(_) => panic!("legacy drop-in must be skipped"),
        Err(err) => err,
    };
    assert!(err.contains("hostApi None unsupported"), "got: {err}");
    assert!(err.contains("skipping load"), "got: {err}");
}

#[test]
fn all_workspace_plugins_load() {
    if require_plugins().is_none() {
        return;
    }
    for id in ["claude", "codex", "opencode"] {
        let p = load_js(id);
        assert_eq!(p.id(), id, "plugin id mismatch for {id}");
    }
}

/// #16 parity (discipline rule #2): a capability method a provider doesn't
/// implement must SKIP-not-recurse and return the SAME sane default on BOTH the
/// native and JS runtimes when there is no native fallback to delegate to. The
/// crash dump showed `JsProvider::native()`-delegation looping forever once a
/// JS-only provider became its own fallback; these two halves prove the JS path
/// now matches the native control instead of overflowing the stack.
mod no_native_fallback_recursion;
