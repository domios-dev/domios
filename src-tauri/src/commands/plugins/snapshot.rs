use serde_json::Value;

use super::cli::{read_audit_events, run_plugin_cli};

/// Scan plugin roots for `plugin.json` manifests, keyed by id. The CLI `plugin list`
/// only reports installed plugins with empty paths, so it can't surface capabilities
/// or workspace-loaded plugins; this gives the on-disk truth the daemon loads.
fn scan_plugin_manifests(
) -> std::collections::HashMap<String, (std::path::PathBuf, Value, Option<String>)> {
    let workspace = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/.."));
    let roots = crate::plugin::js_runtime::default_roots(Some(workspace.as_path()));
    let mut map = std::collections::HashMap::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for e in entries.flatten() {
            let dir = e.path();
            let manifest_path = dir.join("plugin.json");
            let Ok(text) = std::fs::read_to_string(&manifest_path) else {
                continue;
            };
            let Ok(m) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            if let Some(id) = m.get("id").and_then(|v| v.as_str()) {
                map.entry(id.to_string()).or_insert_with(|| {
                    let icon_svg = manifest_icon_svg(&dir, &m);
                    (dir, m, icon_svg)
                });
            }
        }
    }
    map
}

/// Derive `capabilities` (snake_case) + `granted_permissions` + `settings_schema`
/// from a plugin manifest. Absent `capabilities` ⇒ agent provider (mirrors
/// `default_capabilities`). `dir` resolves the relative settings-schema path.
#[cfg(test)]
fn enrich_from_manifest(entry: &mut Value, dir: &std::path::Path, m: &Value) {
    enrich_from_manifest_with_icon(entry, dir, m, None);
}

fn enrich_from_manifest_with_icon(
    entry: &mut Value,
    dir: &std::path::Path,
    m: &Value,
    icon_svg: Option<&str>,
) {
    let caps = m.get("capabilities");
    let agent_provider = match caps {
        Some(c) => c
            .get("agentProvider")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        None => true,
    };
    let cap_bool = |key: &str| {
        caps.and_then(|c| c.get(key))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    };
    // secretBackend/chatPreprocessor/chatBackend may be a bool OR an object — either form means enabled.
    let cap_present = |key: &str| caps.and_then(|c| c.get(key)).is_some_and(|v| !v.is_null());
    // A cap written as an object is enabled; an explicit `false` is not.
    let cap_enabled = |key: &str| {
        caps.and_then(|c| c.get(key))
            .is_some_and(|v| v.as_bool().unwrap_or(v.is_object()))
    };
    let cap_obj = serde_json::json!({
        "agent_provider": agent_provider,
        "secret_backend": cap_enabled("secretBackend"),
        "transcriber": cap_bool("transcriber"),
        "glance_view": cap_bool("glanceView"),
        "status_bubble": cap_bool("statusBubble"),
        "view": cap_bool("view"),
        "chat_backend": cap_bool("chatBackend") || cap_present("chatBackend"),
        "chat_preprocessor": cap_bool("chatPreprocessor") || cap_present("chatPreprocessor"),
    });
    // Persisted grants.yaml override wins over the manifest request. Keys are canonical snake_case, camelCase accepted as fallback.
    let grants = crate::plugin::js_runtime::grants::load_grants(dir);
    let mut granted: Vec<&str> = Vec::new();
    if let Some(p) = m.get("permissions") {
        let allow_nonempty = |key: &str| {
            p.get(key)
                .and_then(|n| n.get("allow"))
                .and_then(|a| a.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false)
        };
        // Toggleable perm: persisted grant override wins, else the manifest request.
        let perm_on = |snake: &str, camel: &str| {
            if let Some(v) = grants.get(snake) {
                return *v;
            }
            p.get(snake)
                .and_then(|v| v.as_bool())
                .or_else(|| p.get(camel).and_then(|v| v.as_bool()))
                .unwrap_or(false)
        };
        if allow_nonempty("network") {
            granted.push("network");
        }
        if allow_nonempty("subprocess") {
            granted.push("subprocess");
        }
        if perm_on("secrets", "secrets") {
            granted.push("secrets");
        }
        if p.get("sendText").and_then(|v| v.as_bool()).unwrap_or(false) {
            granted.push("send_text");
        }
        if perm_on("agent_commands", "agentCommands") {
            granted.push("agent_commands");
        }
        if perm_on("manage_workspaces", "manageWorkspaces") {
            granted.push("manage_workspaces");
        }
        if perm_on("spawn_agents", "spawnAgents") {
            granted.push("spawn_agents");
        }
        if perm_on("analytics", "analytics") {
            granted.push("analytics");
        }
    }
    let settings_schema = m
        .get("settingsSchema")
        .and_then(|v| v.as_str())
        .and_then(|rel| std::fs::read_to_string(dir.join(rel)).ok())
        .and_then(|t| serde_json::from_str::<Value>(&t).ok());

    if let Some(obj) = entry.as_object_mut() {
        remove_untrusted_plugin_icons(obj);
        obj.insert("capabilities".into(), cap_obj);
        obj.insert("granted_permissions".into(), serde_json::json!(granted));
        // Static manifest flag; the live "waiting on reload now" signal is `pending_reload`, set by build_snapshot.
        obj.insert(
            "requires_reload".into(),
            serde_json::json!(m
                .get("requiresReload")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)),
        );
        if let Some(schema) = settings_schema {
            obj.insert("settings_schema".into(), schema);
        }
        if let Some(icon_svg) = icon_svg {
            obj.insert("icon_svg".into(), serde_json::json!(icon_svg));
        }
        if obj.get("display_name").map(|v| v.is_null()).unwrap_or(true) {
            if let Some(dn) = m.get("displayName") {
                obj.insert("display_name".into(), dn.clone());
            }
        }
        if obj.get("version").map(|v| v.is_null()).unwrap_or(true) {
            if let Some(v) = m.get("version") {
                obj.insert("version".into(), v.clone());
            }
        }
        // Static default model from the manifest; live `available_models` are joined separately.
        if let Some(default_model) = m.get("defaultModel") {
            obj.insert("default_model".into(), default_model.clone());
        }
        // Doc surface for the detail tab: description/homepage strings + a bounded README markdown blob.
        if let Some(desc) = m.get("description").and_then(|v| v.as_str()) {
            obj.insert("description".into(), serde_json::json!(desc));
        }
        // Configure-with-AI is opt-in. A description alone does not imply configurability.
        if let Some(help) = m.get("configHelp").and_then(|v| v.as_str()) {
            obj.insert("config_help".into(), serde_json::json!(help));
        }
        if let Some(hp) = m.get("homepage").and_then(|v| v.as_str()) {
            obj.insert("homepage".into(), serde_json::json!(hp));
        }
        let readme_rel = m
            .get("readme")
            .and_then(|v| v.as_str())
            .unwrap_or("README.md");
        if let Some(md) = read_bounded_text(&dir.join(readme_rel)) {
            obj.insert("readme_markdown".into(), serde_json::json!(md));
        }
    }
}

fn remove_untrusted_plugin_icons(entry: &mut serde_json::Map<String, Value>) {
    for key in [
        "icon",
        "icon_svg",
        "iconSvg",
        "icon_color",
        "iconColor",
        "icon_hint",
        "iconHint",
    ] {
        entry.remove(key);
    }
}

/// Read a UTF-8 text file, capped so a runaway README can't bloat the snapshot.
fn read_bounded_text(path: &std::path::Path) -> Option<String> {
    const MAX: usize = 64 * 1024;
    let mut text = std::fs::read_to_string(path).ok()?;
    if text.len() > MAX {
        text.truncate(MAX);
        text.push_str("\n\n… (truncated)");
    }
    Some(text)
}

/// The expensive channel diff runs inside the snapshot's blocking worker.
/// Cold reads complete inline; warm stale reads refresh in the background.
fn fetch_channel_diff() -> Result<Value, String> {
    run_plugin_cli(&["plugin", "channel", "diff", "--json"])
}

/// Synchronously recompute + cache channel drift after explicit plugin/channel actions.
/// The action response must describe post-action state; passive reads stay backgrounded.
pub(super) fn refresh_channel_diff_now() {
    let _ = super::diff_cache::refresh_diff_now(fetch_channel_diff);
}

pub(super) fn build_snapshot(
    last_output: Option<Value>,
    _chatbackends: Option<&crate::chatbackend::ChatBackendRegistry>,
) -> Result<Value, String> {
    let started = std::time::Instant::now();
    let mut plugins = run_plugin_cli(&["plugin", "list", "--json"])?;
    let manifests = scan_plugin_manifests();
    // Plugins installed post-start that declared requiresReload and are waiting to activate.
    let pending_reload: std::collections::HashSet<String> =
        crate::plugin::js_runtime::pending_reload_ids()
            .into_iter()
            .collect();
    if let Some(arr) = plugins.get_mut("plugins").and_then(|p| p.as_array_mut()) {
        let mut seen = std::collections::HashSet::new();
        for entry in arr.iter_mut() {
            if let Some(id) = entry.get("id").and_then(|v| v.as_str()).map(String::from) {
                seen.insert(id.clone());
                if let Some(obj) = entry.as_object_mut() {
                    remove_untrusted_plugin_icons(obj);
                }
                if let Some((dir, m, icon_svg)) = manifests.get(&id) {
                    enrich_from_manifest_with_icon(entry, dir, m, icon_svg.as_deref());
                }
                // the plugin VM mutex and can block tab/session loading.
                if let Some(obj) = entry.as_object_mut() {
                    obj.insert(
                        "pending_reload".into(),
                        serde_json::json!(pending_reload.contains(&id)),
                    );
                }
            }
        }
        // Surface plugins the daemon loaded from disk but the CLI didn't list (dev workspace plugins), so the view is honest.
        for (id, (dir, m, icon_svg)) in &manifests {
            if seen.contains(id) {
                continue;
            }
            // From the workspace source tree, not data_dir: not removable (a remove just reloads), flagged `dev`.
            let mut entry = serde_json::json!({
                "id": id,
                "state": "active",
                "system": false,
                "removable": false,
                "dev": true,
                "install_path": dir.to_string_lossy(),
            });
            enrich_from_manifest_with_icon(&mut entry, dir, m, icon_svg.as_deref());
            arr.push(entry);
        }
    }
    // Best-effort register the default channel so its plugins surface as available; offline failures retry next snapshot.
    let _ = run_plugin_cli(&["plugin", "channel", "ensure-default", "--json"]);
    let channels = run_plugin_cli(&["plugin", "channel", "list", "--json"])?;
    // Cold completion stays inside the blocking snapshot worker.
    let diff = super::diff_cache::cached_diff(fetch_channel_diff);

    let plugins_arr = plugins
        .get("plugins")
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));
    let channels_arr = channels
        .get("channels")
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));
    let diff_arr = diff
        .get("diff")
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));

    let installed_ids: std::collections::HashSet<String> = plugins_arr
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|p| p.get("id").and_then(|v| v.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let marketplace = build_marketplace(
        diff_arr.as_array().map(Vec::as_slice).unwrap_or(&[]),
        channels_arr.as_array().map(Vec::as_slice).unwrap_or(&[]),
        &installed_ids,
    );

    let audit = read_audit_events(80);
    log::debug!(
        "[plugin-snapshot] build_snapshot in {}ms",
        started.elapsed().as_millis()
    );
    Ok(serde_json::json!({
        "plugins": plugins_arr,
        "channels": channels_arr,
        "diff": diff_arr,
        "marketplace": marketplace,
        "audit": audit,
        "lastOutput": last_output,
    }))
}

/// Build marketplace rows from the channel diff: each keeps its diff fields, gains
/// an honest `installed` flag, and is enriched from the channel-cache manifest via
/// `enrich_from_manifest` (the single capability-derivation path).
fn build_marketplace(
    diff: &[Value],
    channels: &[Value],
    installed_ids: &std::collections::HashSet<String>,
) -> Vec<Value> {
    let mut out = Vec::with_capacity(diff.len());
    for row in diff {
        let id = row.get("id").and_then(|v| v.as_str()).unwrap_or_default();
        let channel = row
            .get("channel")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let mut entry = row.clone();
        if let Some(obj) = entry.as_object_mut() {
            remove_untrusted_plugin_icons(obj);
            obj.insert(
                "installed".into(),
                serde_json::json!(installed_ids.contains(id)),
            );
        }
        if let Some((dir, m, icon_svg)) = channel_plugin_manifest(channels, channel, id) {
            enrich_from_manifest_with_icon(&mut entry, &dir, &m, icon_svg.as_deref());
        }
        out.push(entry);
    }
    out
}

/// Locate a channel plugin's on-disk dir + parsed manifest from the channel cache.
/// Reads `<channel.path>/channel.json` to map the plugin id → its relative path.
fn channel_plugin_manifest(
    channels: &[Value],
    channel: &str,
    id: &str,
) -> Option<(std::path::PathBuf, Value, Option<String>)> {
    let channel_path = channels
        .iter()
        .find(|c| c.get("name").and_then(|v| v.as_str()) == Some(channel))
        .and_then(|c| c.get("path").and_then(|v| v.as_str()))
        .filter(|p| !p.is_empty())?;
    let root = std::path::PathBuf::from(channel_path);
    let manifest_text = std::fs::read_to_string(root.join("channel.json")).ok()?;
    let manifest: Value = serde_json::from_str(&manifest_text).ok()?;
    let rel = manifest
        .get("plugins")
        .and_then(|p| p.as_array())?
        .iter()
        .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(id))
        .and_then(|p| {
            p.get("path")
                .and_then(|v| v.as_str())
                .or_else(|| p.get("source").and_then(|v| v.as_str()))
        })?;
    let dir = root.join(rel);
    let plugin_path = dir.join("plugin.json");
    let plugin_text = std::fs::read_to_string(&plugin_path).ok()?;
    let plugin_manifest: Value = serde_json::from_str(&plugin_text).ok()?;
    let icon_svg = manifest_icon_svg(&dir, &plugin_manifest);
    Some((dir, plugin_manifest, icon_svg))
}

// Icons never gate enrichment: a manifest the strict loader rejects still gets its description and capabilities.
fn manifest_icon_svg(dir: &std::path::Path, manifest: &Value) -> Option<String> {
    let relative = manifest.get("icon")?.as_str()?;
    codeterm_plugin::plugin_icon::load_plugin_icon(dir, relative)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            crate::plugin::js_runtime::PluginManifest::load(&root.join("plugin.json")).is_err()
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
            crate::plugin::js_runtime::PluginManifest::load(&root.join("plugin.json")).unwrap();
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
        let _ = std::fs::remove_dir_all(&root);
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
        let installed: std::collections::HashSet<String> =
            ["other".to_string()].into_iter().collect();

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
}
