use super::*;
use crate::agent::provider::{ReadinessStrategy, VerifySubmitStrategy};

#[test]
fn parse_minimal_manifest() {
    let json = r#"{
            "id": "demo",
            "version": "0.1.0",
            "displayName": "Demo CLI",
            "spawn": {
                "readinessStrategy": { "kind": "fixed_delay", "durationMs": 100 },
                "taskDelivery": "pty_input",
                "verifySubmit": { "kind": "none" }
            }
        }"#;
    let m: PluginManifest = serde_json::from_str(json).expect("parse");
    assert_eq!(m.id, "demo");
    assert_eq!(m.display_name, "Demo CLI");
    assert_eq!(m.entry, "dist/plugin.js");
}

#[test]
fn parse_full_manifest() {
    let json = r##"{
            "id": "claude",
            "version": "0.1.0",
            "displayName": "Claude Code",
            "iconHint": "✳",
            "iconColor": "#d97757",
            "binaryName": "claude",
            "cliBinary": "claude",
            "spawn": {
                "readinessStrategy": { "kind": "poll_shell_prompt", "timeoutSecs": 30 },
                "taskDelivery": "inline_flag",
                "taskInputWarmupMs": 0,
                "verifySubmit": { "kind": "tui_input_drained", "timeoutSecs": 15, "idleAfterDrainMs": 800 },
                "sessionIdInjection": "fresh_and_fork",
                "statusBarRows": 3,
                "preservesContextOnResume": true,
                "supportsLazySkills": true,
                "hasHooks": true
            },
            "sessions": {
                "rootDir": "~/.claude/projects",
                "cwdEncoding": "dash_encoded",
                "fileExtension": "jsonl"
            },
            "integration": {
                "kind": "claude_settings_json",
                "settingsPath": "~/.claude/settings.json"
            },
            "rule": {
                "path": "~/.claude/rules/codeterm.md"
            },
            "models": [
                { "id": "sonnet", "displayName": "Claude Sonnet 4.6", "description": "Best for everyday tasks." }
            ],
            "defaultModel": "sonnet",
            "installer": {
                "macos": "curl -fsSL https://claude.ai/install.sh | bash"
            },
            "commands": {
                "compact": "/compact",
                "exit": "/exit",
                "modelSwitchFormat": "/model {model}",
                "autoApproveFlags": ["--dangerously-skip-permissions"],
                "modelLaunchArgsFormat": ["--model", "{model}"]
            },
            "permissions": {
                "transportEvents": ["pty", "events"],
                "sendText": true
            }
        }"##;
    let m: PluginManifest = serde_json::from_str(json).expect("parse");
    assert_eq!(m.id, "claude");
    assert_eq!(m.icon_hint.as_deref(), Some("✳"));
    assert_eq!(m.models.len(), 1);
    assert_eq!(m.default_model.as_deref(), Some("sonnet"));
    assert_eq!(
        m.commands.auto_approve_flags,
        vec!["--dangerously-skip-permissions"]
    );
    assert!(m.spawn.has_hooks);
    assert_eq!(m.spawn.status_bar_rows, 3);
    assert!(matches!(
        m.spawn.verify_submit,
        SerdeVerifySubmit::TuiInputDrained { .. }
    ));
    assert!(matches!(
        m.spawn.session_id_injection,
        SerdeSessionIdInjection::FreshAndFork
    ));
}

#[test]
fn parse_chat_preprocessor_object_carries_match() {
    // §6: chatPreprocessor authored as an OBJECT declaring its slot. The
    // host must accept this (not only the bare bool) or the plugin never
    // loads.
    let json = r#"{
            "id": "chat-preprocessor",
            "version": "0.1.0",
            "displayName": "Chat Preprocessor",
            "capabilities": { "chatPreprocessor": { "match": "text" } }
        }"#;
    let m: PluginManifest = serde_json::from_str(json).expect("parse object form");
    assert!(m.capabilities.chat_preprocessor.enabled());
    assert_eq!(
        m.capabilities.chat_preprocessor.match_slot.as_deref(),
        Some("text"),
        "match slot carried into the capability for the conflict guard"
    );
}

#[test]
fn parse_chat_preprocessor_bool_still_works() {
    let json = r#"{
            "id": "p",
            "version": "0.1.0",
            "displayName": "P",
            "capabilities": { "chatPreprocessor": true }
        }"#;
    let m: PluginManifest = serde_json::from_str(json).expect("parse bool form");
    assert!(m.capabilities.chat_preprocessor.enabled());
    assert_eq!(m.capabilities.chat_preprocessor.match_slot, None);
}

#[test]
fn chat_preprocessor_round_trips_symmetrically() {
    // Disabled must not silently flip to enabled on re-parse (serialize as
    // bool when no slot, object when a slot is declared).
    let cap = Capabilities::default();
    let v = serde_json::to_value(&cap.chat_preprocessor).unwrap();
    assert_eq!(v, serde_json::json!(false));
    let back: ChatPreprocessorCap = serde_json::from_value(v).unwrap();
    assert!(!back.enabled());
}

#[test]
fn parse_chat_backend_bool_is_enabled_not_spawnable() {
    // Bare `chatBackend: true` registers the backend but it is NOT
    // user-spawnable (a host-/agent-driven pane, e.g. the NPC plugin).
    let json = r#"{
            "id": "codeterm-npc",
            "version": "0.1.0",
            "displayName": "Dread NPCs",
            "capabilities": { "chatBackend": true }
        }"#;
    let m: PluginManifest = serde_json::from_str(json).expect("parse bool form");
    assert!(m.capabilities.chat_backend.enabled());
    assert!(
        !m.capabilities.chat_backend_spawnable(),
        "bool form is backend-only, never user-spawnable"
    );
}

#[test]
fn parse_chat_backend_object_spawnable() {
    // `{ spawnable: true }` marks a user-openable open shell (e.g. lmstudio).
    let json = r#"{
            "id": "lmstudio",
            "version": "0.1.0",
            "displayName": "LM Studio",
            "capabilities": { "chatBackend": { "spawnable": true } }
        }"#;
    let m: PluginManifest = serde_json::from_str(json).expect("parse object form");
    assert!(m.capabilities.chat_backend.enabled());
    assert!(
        m.capabilities.chat_backend_spawnable(),
        "object form opts into user-spawnable"
    );
}

#[test]
fn chat_backend_round_trips_symmetrically() {
    // Disabled ⇒ bare `false` (never silently re-enabling); spawnable ⇒
    // object (never silently dropping the flag).
    let disabled = Capabilities::default();
    let v = serde_json::to_value(&disabled.chat_backend).unwrap();
    assert_eq!(v, serde_json::json!(false));

    let spawnable = ChatBackendCap {
        enabled: true,
        spawnable: true,
    };
    let v = serde_json::to_value(&spawnable).unwrap();
    assert_eq!(v, serde_json::json!({ "spawnable": true }));
    let back: ChatBackendCap = serde_json::from_value(v).unwrap();
    assert!(back.enabled() && back.spawnable());
}

#[test]
fn parse_agent_doc_field() {
    // §1.7 agent-doc: codeterm-npc declares `agentDoc`; deny_unknown_fields
    // would reject it without the schema entry → plugin fails to load.
    let json = r#"{
            "id": "codeterm-npc",
            "version": "0.1.0",
            "displayName": "Dread NPCs",
            "agentDoc": "AGENT.md",
            "capabilities": { "chatBackend": true }
        }"#;
    let m: PluginManifest = serde_json::from_str(json).expect("parse agentDoc");
    assert_eq!(m.agent_doc.as_deref(), Some("AGENT.md"));
}

#[test]
fn parse_config_help_field() {
    // Self-setup: every plugin declares `configHelp` for the Configure-with-AI
    // button. deny_unknown_fields would reject it without the schema entry →
    // the plugin (and ALL its capabilities) silently fails to load.
    let json = r#"{
            "id": "transcriber",
            "version": "0.3.0",
            "displayName": "Transcriber",
            "configHelp": "Offline speech-to-text. Knobs: model, language.",
            "capabilities": { "transcriber": true }
        }"#;
    let m: PluginManifest = serde_json::from_str(json).expect("parse configHelp");
    assert_eq!(
        m.config_help.as_deref(),
        Some("Offline speech-to-text. Knobs: model, language.")
    );
}

#[test]
fn parse_snake_case_new_permissions() {
    // codeterm-npc authors the powerful grants in snake_case — the casing the
    // grant store / snapshot / frontend all use. The manifest must accept it.
    let json = r#"{
            "id": "codeterm-npc",
            "version": "0.1.0",
            "displayName": "Dread NPCs",
            "permissions": {
                "manage_workspaces": true,
                "spawn_agents": true,
                "agent_commands": true,
                "secrets": true
            }
        }"#;
    let m: PluginManifest = serde_json::from_str(json).expect("parse snake perms");
    assert!(m.permissions.manage_workspaces);
    assert!(m.permissions.spawn_agents);
    assert!(m.permissions.agent_commands);
    assert!(m.permissions.secrets);
}

#[test]
fn expand_home_handles_tilde() {
    if let Some(home) = dirs::home_dir() {
        assert_eq!(
            PluginManifest::expand_home("~/foo/bar"),
            Some(home.join("foo/bar"))
        );
        assert_eq!(PluginManifest::expand_home("~"), Some(home));
    }
    assert_eq!(
        PluginManifest::expand_home("/abs/path"),
        Some(PathBuf::from("/abs/path"))
    );
}

#[test]
fn readiness_conversion() {
    let s = SerdeReadinessStrategy::PollShellPrompt { timeout_secs: 10 };
    let r: ReadinessStrategy = (&s).into();
    match r {
        ReadinessStrategy::PollShellPrompt { timeout } => {
            assert_eq!(timeout.as_secs(), 10);
        }
        _ => panic!(),
    }
}

#[test]
fn verify_submit_conversion() {
    let s = SerdeVerifySubmit::TuiInputDrained {
        timeout_secs: 15,
        idle_after_drain_ms: 800,
    };
    let v: VerifySubmitStrategy = (&s).into();
    match v {
        VerifySubmitStrategy::TuiInputDrained {
            timeout,
            idle_after_drain,
        } => {
            assert_eq!(timeout.as_secs(), 15);
            assert_eq!(idle_after_drain.as_millis(), 800);
        }
        _ => panic!(),
    }
}
