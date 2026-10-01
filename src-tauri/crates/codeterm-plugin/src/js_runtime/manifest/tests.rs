use super::*;
use crate::provider::{
    BusySubmitStrategy, ContextOwner, ProviderReadinessStrategy, ReadinessStrategy,
    VerifySubmitStrategy,
};

#[test]
fn webhook_receiver_contract_accepts_pebble_object() {
    let json = r#"{
            "id": "pebble",
            "version": "0.1.0",
            "displayName": "Pebble",
            "capabilities": { "webhookReceiver": { "kind": "pebble" } }
        }"#;
    let manifest: PluginManifest = serde_json::from_str(json).expect("parse pebble receiver");
    assert_eq!(
        manifest.capabilities.webhook_receiver,
        Some(WebhookReceiverCap {
            kind: WebhookReceiverKind::Pebble,
            audio: false,
        })
    );
    assert_eq!(
        serde_json::to_value(manifest.capabilities.webhook_receiver).expect("serialize receiver"),
        serde_json::json!({ "kind": "pebble" })
    );
}

#[test]
fn pebble_audio_receiver_manifest_declares_audio_and_round_trips() {
    let json = r#"{
            "id": "pebble",
            "version": "0.1.0",
            "displayName": "Pebble",
            "capabilities": { "webhookReceiver": { "kind": "pebble", "audio": true } }
        }"#;
    let manifest: PluginManifest = serde_json::from_str(json).expect("parse audio receiver");
    let receiver = manifest.capabilities.webhook_receiver.expect("receiver");
    assert!(receiver.audio);
    assert_eq!(
        serde_json::to_value(&receiver).expect("serialize receiver"),
        serde_json::json!({ "kind": "pebble", "audio": true })
    );
    let non_bool = serde_json::json!({
        "id": "pebble", "version": "0.1.0", "displayName": "Pebble",
        "capabilities": { "webhookReceiver": { "kind": "pebble", "audio": "yes" } }
    });
    assert!(serde_json::from_value::<PluginManifest>(non_bool).is_err());
}

#[test]
fn webhook_receiver_contract_rejects_unknown_field() {
    let manifest = serde_json::json!({
        "id": "pebble",
        "version": "0.1.0",
        "displayName": "Pebble",
        "capabilities": {
            "webhookReceiver": { "kind": "pebble", "extra": true }
        }
    });
    let error = serde_json::from_value::<PluginManifest>(manifest)
        .expect_err("unknown receiver fields must be rejected");
    assert!(error.to_string().contains("extra"), "got: {error}");
}

#[test]
fn manifest_unknown_capability_is_ignored_and_known_ones_load() {
    let manifest = PluginManifest::from_json_str(
        r#"{
            "id":"future-capability-fixture",
            "version":"0.1.0",
            "displayName":"Future capability fixture",
            "capabilities":{"decisionModel":true,"futureCapability":{"flag":true}}
        }"#,
    )
    .expect("unknown capabilities are ignored");
    assert!(manifest.capabilities.decision_model);
    assert_eq!(
        manifest
            .capabilities
            .unknown_capabilities()
            .collect::<Vec<_>>(),
        vec!["futureCapability"]
    );
}

#[test]
fn manifest_top_level_unknown_fields_remain_strict() {
    assert!(PluginManifest::from_json_str(
        r#"{
            "id":"strict-fixture",
            "version":"0.1.0",
            "displayName":"Strict fixture",
            "futureField":true
        }"#,
    )
    .is_err());
}

#[test]
fn plugin_icon_svg_load_falls_back_from_item_to_plugin_asset() {
    let root = std::env::temp_dir().join(format!("ct-manifest-icon-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("icons")).unwrap();
    let icon =
        r#"<svg viewBox="0 0 1 1" fill="currentColor"><circle cx="0.5" cy="0.5" r="0.5"/></svg>"#;
    let item_icon = r#"<svg viewBox="0 0 1 1" fill="currentColor"><path d="M0 0"/></svg>"#;
    std::fs::write(root.join("icon.svg"), icon).unwrap();
    std::fs::write(root.join("icons/item.svg"), item_icon).unwrap();
    std::fs::write(
        root.join("plugin.json"),
        serde_json::json!({
            "id": "demo",
            "version": "0.1.0",
            "displayName": "Demo",
            "icon": "icon.svg",
            "capabilities": {
                "webhookReceiver": { "kind": "pebble" },
                "tabMenu": [{
                    "id": "receive",
                    "label": "Receive",
                    "action": "receiverTarget",
                    "icon": "icons/item.svg"
                }]
            }
        })
        .to_string(),
    )
    .unwrap();

    let manifest = PluginManifest::load(&root.join("plugin.json")).unwrap();
    assert_eq!(manifest.icon_svg.as_deref(), Some(icon));
    assert_eq!(
        manifest.capabilities.tab_menu[0].icon_svg.as_deref(),
        Some(item_icon)
    );
    assert_eq!(
        manifest.capabilities.tab_menu[0].icon.as_deref(),
        Some("icons/item.svg")
    );
    std::fs::remove_file(root.join("icons/item.svg")).unwrap();
    let fallback = PluginManifest::load(&root.join("plugin.json")).unwrap();
    assert_eq!(
        fallback.capabilities.tab_menu[0].icon_svg.as_deref(),
        Some(icon)
    );
    std::fs::remove_file(root.join("icon.svg")).unwrap();
    let missing = PluginManifest::load(&root.join("plugin.json")).unwrap();
    assert!(missing.icon_svg.is_none());
    assert!(missing.capabilities.tab_menu[0].icon_svg.is_none());
    let _ = std::fs::remove_dir_all(&root);
}

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
            "icon": "icon.svg",
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
                "modelSwitch": "restart_resume",
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
    assert_eq!(m.icon.as_deref(), Some("icon.svg"));
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
fn secret_backend_bool_declares_no_auto_unlock() {
    let json = r#"{
            "id": "vault",
            "version": "0.1.0",
            "displayName": "Vault",
            "capabilities": { "secretBackend": true }
        }"#;
    let m: PluginManifest = serde_json::from_str(json).expect("parse bool form");
    assert!(m.capabilities.secret_backend.enabled());
    assert!(
        !m.capabilities.secret_backend.auto_unlock(),
        "a bare bool must never grant auto-unlock"
    );
}

#[test]
fn secret_backend_object_declares_auto_unlock() {
    let json = r#"{
            "id": "vault",
            "version": "0.1.0",
            "displayName": "Vault",
            "capabilities": { "secretBackend": { "autoUnlock": true } }
        }"#;
    let m: PluginManifest = serde_json::from_str(json).expect("parse object form");
    assert!(m.capabilities.secret_backend.enabled());
    assert!(m.capabilities.secret_backend.auto_unlock());
}

#[test]
fn secret_backend_disabled_cannot_declare_auto_unlock() {
    let json = r#"{
            "id": "vault",
            "version": "0.1.0",
            "displayName": "Vault",
            "capabilities": { "secretBackend": false }
        }"#;
    let m: PluginManifest = serde_json::from_str(json).expect("parse disabled form");
    assert!(!m.capabilities.secret_backend.enabled());
    assert!(!m.capabilities.secret_backend.auto_unlock());
}

#[test]
fn secret_backend_round_trips_symmetrically() {
    let cap = Capabilities::default();
    let v = serde_json::to_value(&cap.secret_backend).unwrap();
    assert_eq!(v, serde_json::json!(false));
    let back: SecretBackendCap = serde_json::from_value(v).unwrap();
    assert!(!back.enabled());

    let declared = SecretBackendCap {
        enabled: true,
        auto_unlock: true,
    };
    let v = serde_json::to_value(&declared).unwrap();
    assert_eq!(v, serde_json::json!({ "autoUnlock": true }));
    let back: SecretBackendCap = serde_json::from_value(v).unwrap();
    assert!(back.auto_unlock());
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
        context_owner: ContextOwner::Plugin,
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
        context_owner: ContextOwner::Plugin,
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
        context_owner: ContextOwner::Plugin,
    };
    let v = serde_json::to_value(&session_config).unwrap();
    assert_eq!(v, serde_json::json!({ "sessionConfig": true }));
    let back: ChatBackendCap = serde_json::from_value(v).unwrap();
    assert!(back.enabled() && back.session_config());

    let host_context = ChatBackendCap {
        enabled: true,
        spawnable: false,
        watcher: false,
        session_config: false,
        context_owner: ContextOwner::Host,
    };
    let v = serde_json::to_value(&host_context).unwrap();
    assert_eq!(v, serde_json::json!({ "contextOwner": "host" }));
    let back: ChatBackendCap = serde_json::from_value(v).unwrap();
    assert!(back.enabled() && back.context_owner() == ContextOwner::Host);
}

/// Independent reimplementation of the serializer as it stood before
/// `contextOwner` existed. Byte-comparing against it is what proves the new
/// field costs an already-shipped manifest nothing.
fn chat_backend_serialization_before_context_owner(cap: &ChatBackendCap) -> String {
    if cap.enabled() && (cap.spawnable() || cap.watcher() || cap.session_config()) {
        let mut entries: Vec<&str> = Vec::new();
        if cap.spawnable() {
            entries.push(r#""spawnable":true"#);
        }
        if cap.watcher() {
            entries.push(r#""watcher":true"#);
        }
        if cap.session_config() {
            entries.push(r#""sessionConfig":true"#);
        }
        format!("{{{}}}", entries.join(","))
    } else if cap.enabled() {
        "true".to_string()
    } else {
        "false".to_string()
    }
}

fn assert_context_owner_absent_is_byte_equivalent(source: &str, cap: &ChatBackendCap) {
    assert_eq!(
        cap.context_owner(),
        ContextOwner::Plugin,
        "{source}: a manifest without the key must stay provider-owned"
    );
    assert_eq!(
        serde_json::to_string(cap).expect("serialize cap"),
        chat_backend_serialization_before_context_owner(cap),
        "{source}: serialization must be byte-equivalent to the pre-change bytes"
    );
}

#[test]
fn context_owner_absent_manifest_parses_byte_equivalent_to_today() {
    let shipped_root =
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../..")).join("plugins");
    let mut shipped = 0usize;
    for entry in std::fs::read_dir(&shipped_root).expect("shipped plugins dir") {
        let manifest_path = entry.expect("plugin dir entry").path().join("plugin.json");
        if !manifest_path.exists() {
            continue;
        }
        let raw = std::fs::read_to_string(&manifest_path).expect("read shipped manifest");
        let manifest: PluginManifest = serde_json::from_str(&raw)
            .unwrap_or_else(|e| panic!("parse {}: {e}", manifest_path.display()));
        assert_context_owner_absent_is_byte_equivalent(
            &manifest_path.display().to_string(),
            &manifest.capabilities.chat_backend,
        );
        shipped += 1;
    }
    assert!(
        shipped > 0,
        "no shipped manifest was parsed — the fixture set must not be empty"
    );

    for declaration in [
        "true",
        "false",
        r#"{ "spawnable": true }"#,
        r#"{ "watcher": true }"#,
        r#"{ "sessionConfig": true }"#,
        r#"{ "spawnable": true, "watcher": true }"#,
        r#"{ "spawnable": true, "watcher": true, "sessionConfig": true }"#,
    ] {
        let json = format!(
            r#"{{ "id":"cap-shape-fixture","version":"0.1.0","displayName":"Cap shape fixture",
                  "capabilities": {{ "chatBackend": {declaration} }} }}"#
        );
        let manifest: PluginManifest =
            serde_json::from_str(&json).unwrap_or_else(|e| panic!("parse {declaration}: {e}"));
        assert_context_owner_absent_is_byte_equivalent(
            declaration,
            &manifest.capabilities.chat_backend,
        );
    }
}

#[test]
fn parse_chat_backend_context_owner_host() {
    let json = r#"{
            "id": "context-owner-fixture",
            "version": "0.1.0",
            "displayName": "Context owner fixture",
            "capabilities": { "chatBackend": { "spawnable": true, "contextOwner": "host" } }
        }"#;
    let m: PluginManifest = serde_json::from_str(json).expect("parse host context owner");
    assert_eq!(
        m.capabilities.chat_backend.context_owner(),
        ContextOwner::Host
    );
    assert!(m.capabilities.chat_backend_spawnable());

    let explicit_plugin = json.replace(r#""host""#, r#""plugin""#);
    let m: PluginManifest = serde_json::from_str(&explicit_plugin).expect("parse explicit plugin");
    assert_eq!(
        m.capabilities.chat_backend.context_owner(),
        ContextOwner::Plugin
    );
}

#[test]
fn unknown_context_owner_fails_the_parse_instead_of_defaulting() {
    // A typo silently meaning "plugin" would reinstate an unverifiable promise.
    for declared in ["hosr", "Host", "HOST", "native", ""] {
        let json = format!(
            r#"{{ "id":"context-owner-typo-fixture","version":"0.1.0","displayName":"Typo fixture",
                  "capabilities": {{ "chatBackend": {{ "contextOwner": "{declared}" }} }} }}"#
        );
        serde_json::from_str::<PluginManifest>(&json).expect_err(&format!(
            "'{declared}' must fail the parse, never fall back"
        ));
    }
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
        retry_offsets_ms: [200, 500, 900, 1500, 2200, 3000],
    };
    let v: VerifySubmitStrategy = (&s).into();
    match v {
        VerifySubmitStrategy::TuiInputDrained {
            initial_key_delay,
            retry_offsets,
        } => {
            assert_eq!(initial_key_delay, std::time::Duration::from_millis(150));
            assert_eq!(retry_offsets[5], std::time::Duration::from_millis(3000));
        }
        _ => panic!(),
    }
}

#[test]
fn submit_capability_fields_native_js_parity() {
    use crate::provider::AgentProvider;

    let codex: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../../plugins/provider-codex/plugin.json"
    ))
    .unwrap();
    let claude: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../../plugins/provider-claude/plugin.json"
    ))
    .unwrap();
    let codex_submit: SerdeVerifySubmit =
        serde_json::from_value(codex["spawn"]["verifySubmit"].clone()).unwrap();
    let claude_submit: SerdeVerifySubmit =
        serde_json::from_value(claude["spawn"]["verifySubmit"].clone()).unwrap();
    let codex_normalization: crate::provider::TurnEvidenceNormalization =
        serde_json::from_value(codex["spawn"]["turnEvidenceNormalization"].clone()).unwrap();
    let claude_normalization: crate::provider::TurnEvidenceNormalization =
        serde_json::from_value(claude["spawn"]["turnEvidenceNormalization"].clone()).unwrap();
    let codex_strategy = VerifySubmitStrategy::from(&codex_submit);
    let claude_strategy = VerifySubmitStrategy::from(&claude_submit);
    let native_claude = crate::agent_claude::provider::ClaudeProvider::new();

    assert_eq!(
        codex_strategy.initial_key_delay(),
        std::time::Duration::from_millis(300)
    );
    assert_eq!(
        codex_strategy.retry_offsets(),
        [200, 500, 1200, 2200, 4000, 5800].map(std::time::Duration::from_millis)
    );
    assert_eq!(
        claude_strategy,
        native_claude.verify_submit_strategy(),
        "the Claude JS capability matches its native provider strategy"
    );
    assert_eq!(
        claude_normalization,
        native_claude.turn_evidence_normalization(),
        "the Claude JS capability matches its native evidence normalization"
    );
    assert_eq!(
        codex_normalization,
        crate::provider::TurnEvidenceNormalization {
            drop_supplementary_characters: true,
            ..Default::default()
        }
    );
}

#[test]
fn busy_submit_conversion() {
    let strategy = SerdeBusySubmit::Key { key: "tab".into() };
    assert_eq!(
        BusySubmitStrategy::from(&strategy),
        BusySubmitStrategy::Key("tab".into())
    );
}

#[test]
fn hook_event_roster_parses_and_resolves_declared_routes() {
    use crate::provider::{HookEventBinding, HookIngestionRoute, ProviderStateVerdict};

    let json = r#"{
            "id": "roster-demo",
            "version": "0.1.0",
            "displayName": "Roster Demo",
            "spawn": {
                "readinessStrategy": { "kind": "fixed_delay", "durationMs": 0 },
                "taskDelivery": "pty_input",
                "verifySubmit": { "kind": "none" },
                "hasHooks": true,
                "hookEventRoster": [
                    { "event": "Stop", "route": "stop" },
                    { "event": "SessionStart", "route": "session_start" },
                    { "event": "PreCompact", "route": "mem_flush" },
                    { "event": "Notification", "route": "permission" },
                    { "event": "session.idle", "route": "session_state", "verdict": "idle" }
                ]
            }
        }"#;
    let manifest: PluginManifest = serde_json::from_str(json).expect("parse");
    let declared = manifest.spawn.hook_event_roster.expect("roster declared");
    let resolved = resolve_hook_event_roster(&declared).expect("all routes known");
    assert_eq!(
        resolved,
        vec![
            HookEventBinding {
                event: "Stop".into(),
                route: HookIngestionRoute::Stop,
                verdict: None,
            },
            HookEventBinding {
                event: "SessionStart".into(),
                route: HookIngestionRoute::SessionStart,
                verdict: None,
            },
            HookEventBinding {
                event: "PreCompact".into(),
                route: HookIngestionRoute::MemFlush,
                verdict: None,
            },
            HookEventBinding {
                event: "Notification".into(),
                route: HookIngestionRoute::Permission,
                verdict: None,
            },
            HookEventBinding {
                event: "session.idle".into(),
                route: HookIngestionRoute::SessionState,
                verdict: Some(ProviderStateVerdict::Idle),
            },
        ]
    );
}

#[test]
fn hook_event_roster_resolution_rejects_unknown_route() {
    let declared = vec![
        SerdeHookEventBinding {
            event: "Stop".into(),
            route: "stop".into(),
            verdict: None,
        },
        SerdeHookEventBinding {
            event: "Warp".into(),
            route: "warp_core".into(),
            verdict: None,
        },
    ];
    let error = resolve_hook_event_roster(&declared).expect_err("unknown route fails the roster");
    assert!(
        error.contains("Warp") && error.contains("warp_core"),
        "{error}"
    );
}

#[test]
fn permission_request_roster_resolution_rejects_unknown_verdict() {
    let declared = vec![SerdeHookEventBinding {
        event: "session.status".into(),
        route: "session_state".into(),
        verdict: Some("busy".into()),
    }];
    let error = resolve_hook_event_roster(&declared).expect_err("unknown verdict fails the roster");
    assert!(
        error.contains("session.status") && error.contains("busy"),
        "{error}"
    );
}

#[test]
fn manifest_accepts_legacy_icon_fields_shipped_by_channel_plugins() {
    let manifest = PluginManifest::from_json_str(
        r##"{
            "id": "legacy",
            "version": "1.0.0",
            "displayName": "Legacy",
            "description": "Still loads",
            "iconHint": "lock",
            "iconColor": "#175ddc",
            "iconSvg": "<svg viewBox=\"0 0 1 1\"></svg>",
            "capabilities": { "view": true }
        }"##,
    )
    .expect("pre-1.10.21 channel manifests keep loading");
    assert_eq!(manifest.description.as_deref(), Some("Still loads"));
    assert!(
        manifest.icon_svg.is_none(),
        "legacy inline SVG is never trusted as the icon"
    );
}
