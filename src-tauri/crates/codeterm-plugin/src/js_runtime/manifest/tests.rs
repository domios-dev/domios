use super::*;
use crate::provider::{
    BusySubmitStrategy, ProviderReadinessStrategy, ReadinessStrategy, VerifySubmitStrategy,
};

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
                "providerReadiness": { "kind": "structural_composer", "timeoutSecs": 30 },
                "verifySubmit": { "kind": "tui_input_drained" },
                "sessionIdInjection": "fresh_and_fork",
                "statusBarRows": 3,
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
        ProviderReadinessStrategy::from(&m.spawn.provider_readiness),
        ProviderReadinessStrategy::StructuralComposer { timeout }
            if timeout == std::time::Duration::from_secs(30)
    ));
    assert!(matches!(
        m.spawn.session_id_injection,
        SerdeSessionIdInjection::FreshAndFork
    ));
}

#[test]
fn integration_trust_grant_manifest_mapping() {
    let json = r#"{
            "id": "demo",
            "version": "0.1.0",
            "displayName": "Demo CLI",
            "spawn": {
                "readinessStrategy": { "kind": "fixed_delay", "durationMs": 0 },
                "taskDelivery": "pty_input",
                "verifySubmit": { "kind": "none" }
            },
            "integration": {
                "kind": "provider_managed",
                "settingsPath": "~/.demo/hooks.json",
                "trust_grant": { "managedKeys": ["session_start"] }
            }
        }"#;
    let m: PluginManifest = serde_json::from_str(json).expect("parse");
    let grant = m
        .integration
        .as_ref()
        .and_then(|integration| integration.trust_grant.as_ref())
        .expect("trust_grant declaration must map");
    assert_eq!(grant.managed_keys, vec!["session_start".to_string()]);

    let camel = json.replace("trust_grant", "trustGrant");
    let m: PluginManifest = serde_json::from_str(&camel).expect("parse camelCase alias");
    assert!(m
        .integration
        .expect("integration")
        .trust_grant
        .is_some_and(|grant| grant.managed_keys == vec!["session_start".to_string()]));
}

#[test]
fn integration_trust_grant_absent_by_default() {
    let json = r#"{
            "id": "demo",
            "version": "0.1.0",
            "displayName": "Demo CLI",
            "spawn": {
                "readinessStrategy": { "kind": "fixed_delay", "durationMs": 0 },
                "taskDelivery": "pty_input",
                "verifySubmit": { "kind": "none" }
            },
            "integration": {
                "kind": "provider_managed",
                "settingsPath": "~/.demo/hooks.json"
            }
        }"#;
    let m: PluginManifest = serde_json::from_str(json).expect("parse");
    assert!(m.integration.expect("integration").trust_grant.is_none());
}

#[test]
fn retired_task_input_delay_is_rejected() {
    let retired_field = ["taskInput", "W", "armup", "Ms"].concat();
    let mut manifest = serde_json::json!({
        "id": "retired-delay",
        "version": "0.1.0",
        "displayName": "Retired delay",
        "spawn": {
            "readinessStrategy": { "kind": "poll_shell_prompt", "timeoutSecs": 30 },
            "taskDelivery": "pty_input",
            "verifySubmit": { "kind": "none" }
        }
    });
    manifest["spawn"][retired_field.clone()] = serde_json::json!(3000);

    let error = serde_json::from_value::<PluginManifest>(manifest)
        .expect_err("fixed task-input delays must be rejected");
    assert!(error.to_string().contains(&retired_field), "got: {error}");
}

#[test]
fn parse_chat_preprocessor_object_carries_match() {
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
    // bool when no slot, object when a slot is declared).
    let cap = Capabilities::default();
    let v = serde_json::to_value(&cap.chat_preprocessor).unwrap();
    assert_eq!(v, serde_json::json!(false));
    let back: ChatPreprocessorCap = serde_json::from_value(v).unwrap();
    assert!(!back.enabled());
}

#[test]
fn parse_chat_backend_bool_is_enabled_not_spawnable() {
    // user-spawnable (a host-/agent-driven tab, e.g. the NPC plugin).
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
    assert!(
        !m.capabilities.chat_backend_watcher_capable(),
        "bool form does not imply watcher capability"
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
fn parse_chat_backend_object_watcher_without_spawnable() {
    let json = r#"{
            "id": "lmstudio-watcher",
            "version": "0.1.0",
            "displayName": "LM Studio Watcher",
            "capabilities": { "chatBackend": { "watcher": true } }
        }"#;
    let m: PluginManifest = serde_json::from_str(json).expect("parse watcher object form");
    assert!(m.capabilities.chat_backend.enabled());
    assert!(
        !m.capabilities.chat_backend_spawnable(),
        "watcher-capable does not imply user-spawnable"
    );
    assert!(m.capabilities.chat_backend_watcher_capable());
}

#[test]
fn chat_backend_round_trips_symmetrically() {
    // object (never silently dropping the flag).
    let disabled = Capabilities::default();
    let v = serde_json::to_value(&disabled.chat_backend).unwrap();
    assert_eq!(v, serde_json::json!(false));

    let spawnable = ChatBackendCap {
        enabled: true,
        spawnable: true,
        watcher: false,
        session_config: false,
    };
    let v = serde_json::to_value(&spawnable).unwrap();
    assert_eq!(v, serde_json::json!({ "spawnable": true }));
    let back: ChatBackendCap = serde_json::from_value(v).unwrap();
    assert!(back.enabled() && back.spawnable() && !back.watcher());

    let watcher = ChatBackendCap {
        enabled: true,
        spawnable: false,
        watcher: true,
        session_config: false,
    };
    let v = serde_json::to_value(&watcher).unwrap();
    assert_eq!(v, serde_json::json!({ "watcher": true }));
    let back: ChatBackendCap = serde_json::from_value(v).unwrap();
    assert!(back.enabled() && !back.spawnable() && back.watcher());

    let session_config = ChatBackendCap {
        enabled: true,
        spawnable: false,
        watcher: false,
        session_config: true,
    };
    let v = serde_json::to_value(&session_config).unwrap();
    assert_eq!(v, serde_json::json!({ "sessionConfig": true }));
    let back: ChatBackendCap = serde_json::from_value(v).unwrap();
    assert!(back.enabled() && back.session_config());
}

#[test]
fn parse_agent_doc_field() {
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
        initial_key_delay_ms: 150,
    };
    let v: VerifySubmitStrategy = (&s).into();
    match v {
        VerifySubmitStrategy::TuiInputDrained { initial_key_delay } => {
            assert_eq!(initial_key_delay, std::time::Duration::from_millis(150));
        }
        _ => panic!(),
    }
}

#[test]
fn busy_submit_conversion() {
    let strategy = SerdeBusySubmit::Key { key: "tab".into() };
    assert_eq!(
        BusySubmitStrategy::from(&strategy),
        BusySubmitStrategy::Key("tab".into())
    );
}
