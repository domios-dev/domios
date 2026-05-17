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
        // Synthetic plugin: same plugin.js as bundled claude, but a
        // manifest whose rule.path points into our tempdir. Skip the
        // workspace one to avoid touching real ~/.claude.
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
                "rule":{{"path":"{rp}"}}
            }}"#,
            rp = rule_path.to_string_lossy().replace('\\', "/")
        );
        let manifest: crate::plugin::js_runtime::PluginManifest =
            serde_json::from_str(&manifest_json).unwrap();
        let vm = crate::plugin::js_runtime::vm::PluginVm::from_source_with(
            "claude-test",
            plugin_js,
            Some(manifest_json),
        )
        .unwrap();
        let provider = JsProvider::new(manifest, std::sync::Arc::new(vm));

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
    fn launch_command_simple_interactive_form() {
        if !ws_exists() { return; }
        let (_n, j) = pair();
        // JS plugin emits the simple form (no stdin-pipe). Native impl
        // adds OS-specific env wrappers; we don't claim parity there.
        let params = AgentLaunchParams {
            skip_permissions: true,
            ..Default::default()
        };
        let cmd = j.build_launch_command(&params);
        assert!(cmd.starts_with("claude"));
        assert!(cmd.contains("--dangerously-skip-permissions"));
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
}

// ────────────────────────────────────────────────────────────────────
// Loader behaviour
// ────────────────────────────────────────────────────────────────────

#[test]
fn all_four_workspace_plugins_load() {
    if !ws_exists() { return; }
    for id in ["claude", "codex", "opencode", "cursor"] {
        let p = load_js(id);
        assert_eq!(p.id(), id, "plugin id mismatch for {id}");
    }
}
