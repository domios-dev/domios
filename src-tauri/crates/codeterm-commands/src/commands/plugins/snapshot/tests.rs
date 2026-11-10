use super::*;

#[test]
fn load_errors_attach_to_their_row_and_orphans_get_a_row() {
    let mut arr = vec![
        serde_json::json!({"id": "active-plugin", "state": "active"}),
        serde_json::json!({"id": "healthy", "state": "active"}),
    ];
    let error = |id: &str, path: &str| codeterm_service_core::plugin::js_runtime::PluginLoadError {
        id: id.to_string(),
        path: std::path::PathBuf::from(path),
        message: "parse: unknown field `prompt`".to_string(),
        at_ms: 7,
        kept_previous: true,
    };
    attach_load_errors(
        &mut arr,
        vec![
            error("active-plugin", "/p/fixture-active/plugin.json"),
            error("broken", "/p/broken/plugin.json"),
        ],
    );
    assert_eq!(
        arr[0]["load_error"]["message"],
        "parse: unknown field `prompt`"
    );
    assert_eq!(arr[0]["load_error"]["at_ms"], 7);
    assert!(arr[1]["load_error"].is_null());
    assert_eq!(arr.len(), 3);
    assert_eq!(arr[2]["id"], "broken");
    assert_eq!(arr[2]["state"], "error");
    assert!(arr[2]["install_path"]
        .as_str()
        .is_some_and(|path| path.ends_with("broken")));
}

#[test]
fn update_reconciliation_uses_current_install_and_owning_channel() {
    let mut plugins = serde_json::json!([
        {"id":"demo", "version":"1.0.0", "source":{"kind":"channel", "channel":"store"}},
        {"id":"local", "version":"1.0.0", "source":{"kind":"dir"}},
        {"id":"workspace", "version":"1.0.0", "dev":true},
        {"id":"unknown", "version":"1.0.0"}
    ]);
    let diff = serde_json::json!([
        {"id":"demo", "channel":"store", "local_version":"0.1.0", "remote_version":"1.0.0", "payload_version":"1.0.0", "compatible":true, "drift":"update_available"},
        {"id":"demo", "channel":"other", "remote_version":"9.0.0", "payload_version":"9.0.0", "compatible":true},
        {"id":"local", "channel":"store", "remote_version":"9.0.0", "payload_version":"9.0.0", "compatible":true}
    ]);
    reconcile_update_availability(&mut plugins, &diff);
    assert_eq!(plugins[0]["update"]["reason"], "same_version");
    assert_eq!(plugins[1]["update"]["reason"], "local_install");
    assert_eq!(plugins[2]["update"]["reason"], "unknown_source");
    assert_eq!(plugins[3]["update"]["reason"], "unknown_source");
    for plugin in plugins.as_array().unwrap() {
        assert_eq!(plugin["update"]["available"], false);
    }
    plugins[0]["version"] = serde_json::json!("0.1.0");
    reconcile_update_availability(&mut plugins, &diff);
    assert_eq!(plugins[0]["update"]["available"], true);
    assert_eq!(plugins[0]["update"]["remote_version"], "1.0.0");
}

#[test]
fn plugin_icon_svg_manifest_the_strict_loader_rejects_still_enriches() {
    let root = std::env::temp_dir().join(format!("ct-snapshot-lenient-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
            root.join("icon.svg"),
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><circle cx="12" cy="12" r="7"/></svg>"#,
        )
        .unwrap();
    let manifest = serde_json::json!({
        "id": "lenient",
        "version": "0.1.0",
        "displayName": "Lenient",
        "description": "Still described",
        "icon": "icon.svg",
        "capabilities": { "view": true },
        "futureManifestField": true,
    });
    std::fs::write(root.join("plugin.json"), manifest.to_string()).unwrap();
    assert!(
        codeterm_service_core::plugin::js_runtime::PluginManifest::load(&root.join("plugin.json"))
            .is_err()
    );

    let icon_svg = manifest_icon_svg(&root, &manifest);
    assert!(icon_svg
        .as_deref()
        .is_some_and(|svg| svg.contains("circle")));
    let mut entry = serde_json::json!({ "id": "lenient" });
    enrich_from_manifest_with_icon(&mut entry, &root, &manifest, icon_svg.as_deref());
    assert_eq!(entry["description"], "Still described");
    assert_eq!(entry["capabilities"]["view"], true);
    assert!(entry["icon_svg"]
        .as_str()
        .is_some_and(|svg| svg.contains("circle")));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn plugin_icon_svg_snapshot_carries_only_the_sanitized_asset() {
    let root = std::env::temp_dir().join(format!("ct-snapshot-icon-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("icon.svg"),
        r#"<svg viewBox="0 0 1 1" fill="currentColor"><path d="M0 0"/></svg>"#,
    )
    .unwrap();
    std::fs::write(
        root.join("plugin.json"),
        serde_json::json!({
            "id": "demo",
            "version": "0.1.0",
            "displayName": "Demo",
            "icon": "icon.svg",
        })
        .to_string(),
    )
    .unwrap();

    let manifest =
        codeterm_service_core::plugin::js_runtime::PluginManifest::load(&root.join("plugin.json"))
            .unwrap();
    let mut entry = serde_json::json!({ "id": "demo" });
    enrich_from_manifest_with_icon(
        &mut entry,
        &root,
        &serde_json::json!({ "id": "demo", "displayName": "Demo" }),
        manifest.icon_svg.as_deref(),
    );
    assert_eq!(entry["icon_svg"], serde_json::json!(manifest.icon_svg));
    entry["iconSvg"] = serde_json::json!("<svg>raw</svg>");
    entry["icon"] = serde_json::json!("https://example.test/icon.svg");
    entry["icon_color"] = serde_json::json!("#f00");
    entry["icon_hint"] = serde_json::json!("raw");
    enrich_from_manifest_with_icon(&mut entry, &root, &serde_json::json!({}), None);
    assert!(entry.get("icon").is_none());
    assert!(entry.get("icon_svg").is_none());
    assert!(entry.get("iconSvg").is_none());
    assert!(entry.get("icon_color").is_none());
    assert!(entry.get("icon_hint").is_none());
    enrich_from_manifest_with_icon(
        &mut entry,
        &root,
        &serde_json::json!({ "iconColor": "#d97757" }),
        None,
    );
    assert_eq!(entry["icon_color"], serde_json::json!("#d97757"));
    enrich_from_manifest_with_icon(
        &mut entry,
        &root,
        &serde_json::json!({ "iconColor": "red; background: url(x)" }),
        None,
    );
    assert!(entry.get("icon_color").is_none());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn enrich_separates_declared_network_from_extra_grants() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("grants.yaml"),
        "network_allow:\n  - extra.example.test\n",
    )
    .unwrap();
    let mut entry = serde_json::json!({ "id": "demo" });
    enrich_from_manifest(
        &mut entry,
        dir.path(),
        &serde_json::json!({
            "permissions": { "network": { "allow": ["declared.example.test"] } }
        }),
    );
    assert_eq!(entry["network_requested"], true);
    assert_eq!(
        entry["declared_network_allow"],
        serde_json::json!(["declared.example.test"])
    );
    assert_eq!(
        entry["effective_network_allow"],
        serde_json::json!(["declared.example.test", "extra.example.test"])
    );
    enrich_from_manifest(&mut entry, dir.path(), &serde_json::json!({}));
    assert_eq!(entry["network_requested"], false);
    assert_eq!(entry["declared_network_allow"], serde_json::json!([]));
}

#[test]
fn enrich_reads_description_homepage_and_readme() {
    let dir = std::env::temp_dir().join(format!("ct-enrich-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = serde_json::json!({
        "id": "demo",
        "version": "0.2.0",
        "displayName": "Demo",
        "description": "Does a demo thing.",
        "pitch": "Demo things happen for you.",
        "homepage": "https://example.com/demo",
        "capabilities": { "statusBubble": true },
    });
    std::fs::write(dir.join("README.md"), "# Demo\n\nUseful docs here.").unwrap();

    let mut entry = serde_json::json!({ "id": "demo" });
    enrich_from_manifest(&mut entry, &dir, &manifest);

    assert_eq!(
        entry["description"],
        serde_json::json!("Does a demo thing.")
    );
    assert_eq!(
        entry["pitch"],
        serde_json::json!("Demo things happen for you.")
    );
    assert_eq!(
        entry["homepage"],
        serde_json::json!("https://example.com/demo")
    );
    assert!(entry["readme_markdown"]
        .as_str()
        .unwrap()
        .contains("Useful docs here."));
    assert_eq!(
        entry["capabilities"]["status_bubble"],
        serde_json::json!(true)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn enrich_exposes_only_explicit_config_help() {
    let dir = std::env::temp_dir().join(format!("ct-confhelp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    // configHelp present → carried verbatim as config_help.
    let with_help = serde_json::json!({
        "id": "local-shell",
        "description": "Local LLM via LM Studio.",
        "configHelp": "Set `baseUrl` and `model` to point at your LM Studio server.",
    });
    let mut entry = serde_json::json!({ "id": "local-shell" });
    enrich_from_manifest(&mut entry, &dir, &with_help);
    assert_eq!(
        entry["config_help"],
        serde_json::json!("Set `baseUrl` and `model` to point at your LM Studio server.")
    );

    // Description-only plugins are not AI-configurable.
    let no_help = serde_json::json!({
        "id": "git",
        "description": "Shows git status in the Tab bubble.",
    });
    let mut entry2 = serde_json::json!({ "id": "git" });
    enrich_from_manifest(&mut entry2, &dir, &no_help);
    assert!(entry2.get("config_help").is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn enrich_emits_chat_caps_and_snake_case_grants() {
    let dir = std::env::temp_dir().join(format!("ct-caps-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = serde_json::json!({
        "id": "codeterm-npc",
        "version": "0.1.0",
        "displayName": "Dread NPCs",
        "capabilities": { "chatBackend": true, "chatPreprocessor": { "match": "text" } },
        "permissions": {
            "manage_workspaces": true,
            "spawn_agents": true,
            "agent_commands": true,
            "secrets": true,
            "analytics": true
        }
    });
    let mut entry = serde_json::json!({ "id": "codeterm-npc" });
    enrich_from_manifest(&mut entry, &dir, &manifest);

    assert_eq!(
        entry["capabilities"]["chat_backend"],
        serde_json::json!(true)
    );
    assert_eq!(
        entry["capabilities"]["chat_preprocessor"],
        serde_json::json!(true),
        "object-form chatPreprocessor still reads as enabled"
    );
    let granted: Vec<String> = entry["granted_permissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    for want in [
        "manage_workspaces",
        "spawn_agents",
        "agent_commands",
        "secrets",
        "analytics",
    ] {
        assert!(
            granted.contains(&want.to_string()),
            "missing {want}: {granted:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn enrich_granted_permissions_reflect_grants_yaml() {
    let dir = std::env::temp_dir().join(format!("ct-grant-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // User revoked spawn_agents (manifest requested it) → snapshot must honor it.
    std::fs::write(dir.join("grants.yaml"), "spawn_agents: false\n").unwrap();
    let manifest = serde_json::json!({
        "id": "codeterm-npc",
        "version": "0.1.0",
        "displayName": "Dread NPCs",
        "permissions": { "manage_workspaces": true, "spawn_agents": true }
    });
    let mut entry = serde_json::json!({ "id": "codeterm-npc" });
    enrich_from_manifest(&mut entry, &dir, &manifest);
    let granted: Vec<String> = entry["granted_permissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(granted.contains(&"manage_workspaces".to_string()));
    assert!(
        !granted.contains(&"spawn_agents".to_string()),
        "revoked grant must not appear: {granted:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn enrich_exposes_default_model_from_manifest() {
    let dir = std::env::temp_dir().join(format!("ct-defmodel-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = serde_json::json!({
        "id": "lmstudio",
        "displayName": "LM Studio",
        "defaultModel": "qwen-2.5",
        "capabilities": { "chatBackend": { "spawnable": true } },
    });
    let mut entry = serde_json::json!({ "id": "lmstudio" });
    enrich_from_manifest(&mut entry, &dir, &manifest);
    assert_eq!(entry["default_model"], serde_json::json!("qwen-2.5"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn marketplace_marks_installed_and_pulls_channel_metadata() {
    let root = std::env::temp_dir().join(format!("ct-mp-{}", std::process::id()));
    let plugin_dir = root.join("demo");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(
        root.join("channel.json"),
        serde_json::json!({
            "schemaVersion": 1,
            "name": "demochan",
            "plugins": [{ "id": "demo", "path": "demo", "version": "1.0.0" }],
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        plugin_dir.join("plugin.json"),
        serde_json::json!({
            "id": "demo",
            "version": "1.0.0",
            "displayName": "Demo Plugin",
            "description": "A marketplace demo.",
        })
        .to_string(),
    )
    .unwrap();

    let channels = vec![serde_json::json!({
        "name": "demochan",
        "path": root.to_string_lossy(),
    })];
    let diff = vec![
        serde_json::json!({ "channel": "demochan", "id": "demo", "drift": "not_installed", "compatible": true }),
        serde_json::json!({ "channel": "demochan", "id": "other", "drift": "update_available", "compatible": true }),
    ];
    let installed: std::collections::HashSet<String> = ["other".to_string()].into_iter().collect();

    let market = build_marketplace(&diff, &channels, &installed);
    assert_eq!(market.len(), 2);
    assert_eq!(market[0]["id"], serde_json::json!("demo"));
    assert_eq!(market[0]["installed"], serde_json::json!(false));
    assert_eq!(market[0]["display_name"], serde_json::json!("Demo Plugin"));
    assert_eq!(
        market[0]["description"],
        serde_json::json!("A marketplace demo.")
    );
    // Second row isn't in the channel cache → still present, installed flag honest.
    assert_eq!(market[1]["installed"], serde_json::json!(true));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn enrich_reports_graph_nodes_capability() {
    let dir = std::env::temp_dir().join(format!("ct-graph-nodes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let manifest = serde_json::json!({
        "id": "grapher",
        "version": "1.0.0",
        "displayName": "Grapher",
        "capabilities": {
            "graphNodes": {
                "converters": [{ "from": "video", "to": "audio" }],
                "eventSources": [{ "kind": "fixture.event", "title": "Fixture" }]
            }
        },
    });
    let mut entry = serde_json::json!({ "id": "grapher" });
    enrich_from_manifest_with_icon(&mut entry, &dir, &manifest, None);
    assert_eq!(
        entry["capabilities"]["graph_nodes"],
        serde_json::json!(true)
    );
    assert_eq!(
        entry["capabilities"]["agent_provider"],
        serde_json::json!(false)
    );

    let plain = serde_json::json!({ "id": "plain", "version": "1.0.0", "displayName": "Plain" });
    let mut plain_entry = serde_json::json!({ "id": "plain" });
    enrich_from_manifest_with_icon(&mut plain_entry, &dir, &plain, None);
    assert_eq!(
        plain_entry["capabilities"]["graph_nodes"],
        serde_json::json!(false)
    );

    let disabled = serde_json::json!({
        "id": "off",
        "version": "1.0.0",
        "displayName": "Off",
        "capabilities": { "graphNodes": false },
    });
    let mut disabled_entry = serde_json::json!({ "id": "off" });
    enrich_from_manifest_with_icon(&mut disabled_entry, &dir, &disabled, None);
    assert_eq!(
        disabled_entry["capabilities"]["graph_nodes"],
        serde_json::json!(false)
    );
    let _ = std::fs::remove_dir_all(&dir);
}
#[test]
fn malformed_manifest_scalars_are_absent_and_valid_strings_survive() {
    let dir = tempfile::tempdir().unwrap();
    let mut metadata = Metadata::default();
    enrich_metadata(
        &mut metadata,
        dir.path(),
        &serde_json::json!({"displayName":{"private":"value"},"version":42,"defaultModel":["bad"]}),
        None,
    );
    let malformed = serde_json::json!(metadata);
    for key in ["display_name", "version", "default_model"] {
        assert!(malformed.get(key).is_none(), "{key}");
    }
    enrich_metadata(
        &mut metadata,
        dir.path(),
        &serde_json::json!({"displayName":"Example","version":"1.0.0","defaultModel":"model"}),
        None,
    );
    let valid = serde_json::json!(metadata);
    assert_eq!(valid["display_name"], "Example");
    assert_eq!(valid["version"], "1.0.0");
    assert_eq!(valid["default_model"], "model");
}
