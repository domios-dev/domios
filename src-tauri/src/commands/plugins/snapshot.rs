use serde_json::Value;

use super::cli::{read_audit_events, run_plugin_cli};

pub(crate) fn snapshot(
    chatbackends: Option<&crate::chatbackend::ChatBackendRegistry>,
) -> Result<Value, String> {
    build_snapshot(None, chatbackends)
}

/// Scan the real plugin roots (workspace `plugins/` in dev + `data_dir/plugins`)
/// for `plugin.json` manifests, keyed by id. The CLI `plugin list` only reports
/// *installed* plugins (data_dir + system channel) and leaves their path empty,
/// so it can't surface capabilities or workspace-loaded capability plugins. This
/// gives us the on-disk truth the daemon actually loads.
fn scan_plugin_manifests() -> std::collections::HashMap<String, (std::path::PathBuf, Value)> {
    let workspace = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/.."));
    let roots = crate::plugin::js_runtime::default_roots(Some(workspace.as_path()));
    let mut map = std::collections::HashMap::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for e in entries.flatten() {
            let dir = e.path();
            let Ok(text) = std::fs::read_to_string(dir.join("plugin.json")) else {
                continue;
            };
            let Ok(m) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            if let Some(id) = m.get("id").and_then(|v| v.as_str()) {
                map.entry(id.to_string()).or_insert((dir, m));
            }
        }
    }
    map
}

/// Derive `capabilities` (snake_case) + `granted_permissions` + `settings_schema`
/// from a plugin manifest. Absent `capabilities` ⇒ agent provider (mirrors
/// `default_capabilities`). `dir` resolves the relative settings-schema path.
fn enrich_from_manifest(entry: &mut Value, dir: &std::path::Path, m: &Value) {
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
    // chatPreprocessor and chatBackend may each be a bool OR an object
    // (`{ match }` / `{ spawnable }`) — either form means enabled.
    let cap_present = |key: &str| caps.and_then(|c| c.get(key)).is_some_and(|v| !v.is_null());
    let cap_obj = serde_json::json!({
        "agent_provider": agent_provider,
        "secret_backend": cap_bool("secretBackend"),
        "transcriber": cap_bool("transcriber"),
        "glance_view": cap_bool("glanceView"),
        "status_bubble": cap_bool("statusBubble"),
        "view": cap_bool("view"),
        "chat_backend": cap_bool("chatBackend") || cap_present("chatBackend"),
        "chat_preprocessor": cap_bool("chatPreprocessor") || cap_present("chatPreprocessor"),
    });
    // Effective grants reflect the user's persisted grants.yaml override (§1.10),
    // which wins over the manifest request. Keys are canonical snake_case (what
    // grants.yaml / host_api / the frontend chips all use); accept camelCase as a
    // fallback for manifests authored that way.
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
        // A toggleable perm: persisted grant override wins, else manifest
        // request (snake_case canonical, camelCase fallback).
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
    }
    let settings_schema = m
        .get("settingsSchema")
        .and_then(|v| v.as_str())
        .and_then(|rel| std::fs::read_to_string(dir.join(rel)).ok())
        .and_then(|t| serde_json::from_str::<Value>(&t).ok());

    if let Some(obj) = entry.as_object_mut() {
        obj.insert("capabilities".into(), cap_obj);
        obj.insert("granted_permissions".into(), serde_json::json!(granted));
        // §1.8: a plugin declaring requiresReload can't hot-activate. The static
        // flag is the manifest value; the live "is it actually waiting on a
        // reload right now" signal is `pending_reload`, set by build_snapshot.
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
        // Real icon from the manifest — the providers already ship SVGs. Carry
        // icon_svg / icon_color / icon_hint so the UI renders them, not emoji.
        for (src, dst) in [
            ("iconSvg", "icon_svg"),
            ("iconColor", "icon_color"),
            ("iconHint", "icon_hint"),
        ] {
            if let Some(v) = m.get(src) {
                obj.insert(dst.into(), v.clone());
            }
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
        // chatBackend catalog hint: the manifest may declare a default model the
        // shell opens with. Live `available_models` are joined separately from the
        // registry (`enrich_chat_backend_models`); this is the static default.
        if let Some(default_model) = m.get("defaultModel") {
            obj.insert("default_model".into(), default_model.clone());
        }
        // Documentation surface for the detail pane. `description`/`homepage` are
        // plain manifest strings; `readme` is a relative markdown path (default
        // README.md) loaded into a bounded `readme_markdown` field so the UI can
        // render docs without a second round-trip.
        if let Some(desc) = m.get("description").and_then(|v| v.as_str()) {
            obj.insert("description".into(), serde_json::json!(desc));
        }
        // Agent-facing help for the universal "Configure with AI" button (Part D).
        // Manifest `configHelp` is canonical; fall back to `description` so every
        // plugin yields some text for the safety-wrapped configure task.
        if let Some(help) = m
            .get("configHelp")
            .and_then(|v| v.as_str())
            .or_else(|| m.get("description").and_then(|v| v.as_str()))
        {
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

pub(super) fn build_snapshot(
    last_output: Option<Value>,
    chatbackends: Option<&crate::chatbackend::ChatBackendRegistry>,
) -> Result<Value, String> {
    let mut plugins = run_plugin_cli(&["plugin", "list", "--json"])?;
    let manifests = scan_plugin_manifests();
    // Plugins installed after daemon start that declared requiresReload and so
    // are waiting on a reload to activate (§1.8) — read from the live watcher.
    let pending_reload: std::collections::HashSet<String> =
        crate::plugin::js_runtime::pending_reload_ids()
            .into_iter()
            .collect();
    if let Some(arr) = plugins.get_mut("plugins").and_then(|p| p.as_array_mut()) {
        let mut seen = std::collections::HashSet::new();
        for entry in arr.iter_mut() {
            if let Some(id) = entry.get("id").and_then(|v| v.as_str()).map(String::from) {
                seen.insert(id.clone());
                if let Some((dir, m)) = manifests.get(&id) {
                    enrich_from_manifest(entry, dir, m);
                }
                enrich_chat_backend_models(entry, chatbackends);
                if let Some(obj) = entry.as_object_mut() {
                    obj.insert(
                        "pending_reload".into(),
                        serde_json::json!(pending_reload.contains(&id)),
                    );
                }
            }
        }
        // Surface plugins the daemon loaded from disk but the CLI didn't list
        // (e.g. workspace capability plugins in dev) so the Extensions view is honest.
        for (id, (dir, m)) in &manifests {
            if seen.contains(id) {
                continue;
            }
            // Loaded from the workspace `plugins/` source tree, not data_dir — the
            // manager can't `remove` it (it would just reload), so it is not
            // removable, and it's flagged `dev` so the UI marks it as a
            // workspace/dev plugin rather than a marketplace install. (In prod
            // these same plugins come from the CLI list as data_dir installs and
            // ARE removable.)
            let mut entry = serde_json::json!({
                "id": id,
                "state": "active",
                "system": false,
                "removable": false,
                "dev": true,
                "install_path": dir.to_string_lossy(),
            });
            enrich_from_manifest(&mut entry, dir, m);
            enrich_chat_backend_models(&mut entry, chatbackends);
            arr.push(entry);
        }
    }
    // Best-effort register the default marketplace channel so its plugins
    // surface as available (register-only — nothing is installed). Offline or
    // first-run failures are non-fatal; retried on the next snapshot.
    let _ = run_plugin_cli(&["plugin", "channel", "ensure-default", "--json"]);
    let channels = run_plugin_cli(&["plugin", "channel", "list", "--json"])?;
    let diff = run_plugin_cli(&["plugin", "channel", "diff", "--json"]).unwrap_or_else(|e| {
        serde_json::json!({
            "diff": [],
            "error": e,
        })
    });

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
    Ok(serde_json::json!({
        "plugins": plugins_arr,
        "channels": channels_arr,
        "diff": diff_arr,
        "marketplace": marketplace,
        "audit": audit,
        "lastOutput": last_output,
    }))
}

/// Build normalized marketplace rows from the channel diff. Each row keeps its
/// diff fields (`channel`, `id`, versions, `drift`, `compatible`, …), gains an
/// honest `installed` flag, and is enriched with the channel-cache manifest
/// metadata (icon, display name, capabilities, description, readme) so the UI can
/// show useful detail for plugins that aren't installed yet. Channel metadata is
/// read from the on-disk channel cache (`channels[*].path`), reusing
/// `enrich_from_manifest` — the single capability-derivation path.
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
            obj.insert(
                "installed".into(),
                serde_json::json!(installed_ids.contains(id)),
            );
        }
        if let Some((dir, m)) = channel_plugin_manifest(channels, channel, id) {
            enrich_from_manifest(&mut entry, &dir, &m);
        }
        out.push(entry);
    }
    out
}

/// Enrich a chatBackend plugin entry with its live `available_models` (from the
/// registered provider's `list_models()`). Core asks the capability by the
/// entry's own id — never a hardcoded plugin id. A non-chatBackend entry, an
/// absent registry, or a plugin not registered in the registry is a no-op, so a
/// shell with its server offline simply yields no models (the picker falls back).
fn enrich_chat_backend_models(
    entry: &mut Value,
    chatbackends: Option<&crate::chatbackend::ChatBackendRegistry>,
) {
    let Some(registry) = chatbackends else {
        return;
    };
    let is_chat_backend = entry
        .get("capabilities")
        .and_then(|c| c.get("chat_backend"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !is_chat_backend {
        return;
    }
    let Some(id) = entry.get("id").and_then(|v| v.as_str()).map(String::from) else {
        return;
    };
    let Some(provider) = registry.get(&id) else {
        return;
    };
    let Ok(models) = provider.list_models() else {
        return;
    };
    if let Some(obj) = entry.as_object_mut() {
        obj.insert(
            "available_models".into(),
            serde_json::to_value(models).unwrap_or_default(),
        );
    }
}

/// Locate a channel plugin's on-disk dir + parsed manifest from the channel cache.
/// Reads `<channel.path>/channel.json` to map the plugin id → its relative path.
fn channel_plugin_manifest(
    channels: &[Value],
    channel: &str,
    id: &str,
) -> Option<(std::path::PathBuf, Value)> {
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
    let plugin_text = std::fs::read_to_string(dir.join("plugin.json")).ok()?;
    let plugin_manifest: Value = serde_json::from_str(&plugin_text).ok()?;
    Some((dir, plugin_manifest))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn enrich_exposes_config_help_with_description_fallback() {
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

        // configHelp absent → fall back to description so the magic button always
        // has agent-facing text.
        let no_help = serde_json::json!({
            "id": "git",
            "description": "Shows git status in the pane bubble.",
        });
        let mut entry2 = serde_json::json!({ "id": "git" });
        enrich_from_manifest(&mut entry2, &dir, &no_help);
        assert_eq!(
            entry2["config_help"],
            serde_json::json!("Shows git status in the pane bubble.")
        );

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
                "secrets": true
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
    fn enrich_chat_backend_models_pulls_live_list_models() {
        use crate::agent::provider::ModelInfo;
        use crate::chatbackend::{ChatBackendError, ChatBackendProvider, ChatBackendRegistry};
        use std::sync::Arc;

        struct ModelShell;
        impl ChatBackendProvider for ModelShell {
            fn id(&self) -> &str {
                "lmstudio"
            }
            fn open_session(&self, _: serde_json::Value) -> Result<String, ChatBackendError> {
                Ok("s".into())
            }
            fn send_message(&self, _: &str, _: &str) -> Result<(), ChatBackendError> {
                Ok(())
            }
            fn poll(
                &self,
                _: &str,
                _: Option<&str>,
            ) -> Result<crate::agent::chat_types::StructuredChat, ChatBackendError> {
                Ok(Default::default())
            }
            fn close_session(&self, _: &str) -> Result<(), ChatBackendError> {
                Ok(())
            }
            fn list_models(&self) -> Result<Vec<ModelInfo>, ChatBackendError> {
                Ok(vec![ModelInfo {
                    id: "qwen-2.5".into(),
                    display_name: "Qwen 2.5".into(),
                    description: None,
                    available: None,
                }])
            }
        }

        let registry = ChatBackendRegistry::new();
        registry.register("lmstudio", Arc::new(ModelShell));

        // A chatBackend entry resolves its live models by its own id.
        let mut entry = serde_json::json!({
            "id": "lmstudio",
            "capabilities": { "chat_backend": true },
        });
        enrich_chat_backend_models(&mut entry, Some(&registry));
        assert_eq!(
            entry["available_models"][0]["id"],
            serde_json::json!("qwen-2.5")
        );
        assert_eq!(
            entry["available_models"][0]["display_name"],
            serde_json::json!("Qwen 2.5")
        );

        // A non-chatBackend entry is untouched (no hardcoded id matching).
        let mut other = serde_json::json!({
            "id": "lmstudio",
            "capabilities": { "chat_backend": false },
        });
        enrich_chat_backend_models(&mut other, Some(&registry));
        assert!(other.get("available_models").is_none());

        // No registry ⇒ no-op (the snapshot still builds without a live registry).
        let mut no_reg = serde_json::json!({
            "id": "lmstudio",
            "capabilities": { "chat_backend": true },
        });
        enrich_chat_backend_models(&mut no_reg, None);
        assert!(no_reg.get("available_models").is_none());
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
