use std::process::Command;

use serde::Deserialize;
use serde_json::Value;

const PROTECTED_PLUGIN_CHANNELS: [&str; 2] = ["codeterm-core", "codeterm-plugins"];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManagerAction {
    pub kind: String,
    pub plugin_id: Option<String>,
    pub channel: Option<String>,
    pub url: Option<String>,
    pub name: Option<String>,
    #[serde(default)]
    pub force: bool,
}

#[tauri::command]
pub fn plugin_manager_snapshot() -> Result<Value, String> {
    build_snapshot(None)
}

#[tauri::command]
pub fn plugin_manager_action(action: PluginManagerAction) -> Result<Value, String> {
    run_action(action)
}

/// Render (or act on) a plugin's glance view. `action == None` renders fresh;
/// `Some(id)` dispatches a button action and returns the refreshed view.
#[tauri::command]
pub async fn plugin_glance(
    svc: tauri::State<'_, crate::service::ServiceState>,
    plugin_id: String,
    action: Option<String>,
    ctx: Option<Value>,
) -> Result<Value, String> {
    glance_render(
        &svc.glance_registry,
        &plugin_id,
        action.as_deref(),
        ctx.unwrap_or(Value::Null),
    )
    .await
}

/// Shared by the Tauri command and the web-mirror REST route.
pub(crate) async fn glance_render(
    registry: &crate::glance::GlanceRegistry,
    plugin_id: &str,
    action: Option<&str>,
    ctx: Value,
) -> Result<Value, String> {
    let provider = registry
        .get(plugin_id)
        .ok_or_else(|| format!("no glance provider for plugin '{plugin_id}'"))?;
    let view = match action {
        Some(a) => provider.action(a, ctx).await,
        None => provider.render(ctx).await,
    }
    .map_err(|e| e.to_string())?;
    serde_json::to_value(view).map_err(|e| e.to_string())
}

/// Poll a single plugin's status bubble for a pane context. `null` = hide.
#[tauri::command]
pub async fn plugin_bubble(
    svc: tauri::State<'_, crate::service::ServiceState>,
    plugin_id: String,
    ctx: Option<Value>,
) -> Result<Value, String> {
    bubble_poll(&svc.bubble_registry, &plugin_id, ctx.unwrap_or(Value::Null)).await
}

/// Shared by the Tauri command and the web-mirror REST route.
pub(crate) async fn bubble_poll(
    registry: &crate::bubble::BubbleRegistry,
    plugin_id: &str,
    ctx: Value,
) -> Result<Value, String> {
    let provider = registry
        .get(plugin_id)
        .ok_or_else(|| format!("no bubble provider for plugin '{plugin_id}'"))?;
    let pill = provider.bubble(ctx).await.map_err(|e| e.to_string())?;
    serde_json::to_value(pill).map_err(|e| e.to_string())
}

/// The iframe `ct.invoke(method, args)` bridge endpoint → the plugin's `viewCall`.
#[tauri::command]
pub async fn plugin_view_call(
    svc: tauri::State<'_, crate::service::ServiceState>,
    plugin_id: String,
    method: String,
    args: Option<Value>,
) -> Result<Value, String> {
    view_call(
        &svc.view_registry,
        &plugin_id,
        &method,
        args.unwrap_or(Value::Null),
    )
    .await
}

/// Shared by the Tauri command and the web-mirror REST route.
pub(crate) async fn view_call(
    registry: &crate::view_host::ViewRegistry,
    plugin_id: &str,
    method: &str,
    args: Value,
) -> Result<Value, String> {
    let provider = registry
        .get(plugin_id)
        .ok_or_else(|| format!("no view provider for plugin '{plugin_id}'"))?;
    provider.call(method, args).await.map_err(|e| e.to_string())
}

/// Run a plugin-manager action and return the refreshed snapshot. Shared by the
/// Tauri command and the web-mirror REST route (`POST /api/plugins/action`).
pub(crate) fn run_action(action: PluginManagerAction) -> Result<Value, String> {
    if action.kind == "remove-channel" {
        let channel = action
            .channel
            .clone()
            .ok_or("remove-channel requires channel")?;
        if is_protected_channel(&channel) {
            return Err(format!("channel '{channel}' is protected"));
        }
        remove_channel_plugins(&channel, action.force)?;
    }
    let args = plugin_action_args(&action)?;
    let output = run_plugin_cli(&args)?;
    build_snapshot(Some(output))
}

fn is_protected_channel(channel: &str) -> bool {
    PROTECTED_PLUGIN_CHANNELS.contains(&channel)
}

fn remove_channel_plugins(channel: &str, force: bool) -> Result<(), String> {
    let installed = run_plugin_cli(&["plugin", "list", "--json"])?;
    let ids: Vec<String> = installed
        .get("plugins")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter(|p| {
            p.get("source")
                .and_then(|s| s.get("channel"))
                .and_then(|v| v.as_str())
                == Some(channel)
        })
        .filter_map(|p| p.get("id").and_then(|v| v.as_str()).map(String::from))
        .collect();

    for id in ids {
        let mut args = vec!["plugin".to_string(), "remove".to_string(), id];
        if force {
            args.push("--force".to_string());
        }
        args.push("--json".to_string());
        run_plugin_cli(&args)?;
    }
    Ok(())
}

/// Build the plugin snapshot. Shared by the Tauri command and the web-mirror
/// REST route (`GET /api/plugins/snapshot`).
pub(crate) fn snapshot() -> Result<Value, String> {
    build_snapshot(None)
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
    let cap_obj = serde_json::json!({
        "agent_provider": agent_provider,
        "secret_backend": cap_bool("secretBackend"),
        "transcriber": cap_bool("transcriber"),
        "glance_view": cap_bool("glanceView"),
        "status_bubble": cap_bool("statusBubble"),
        "view": cap_bool("view"),
    });
    let mut granted: Vec<&str> = Vec::new();
    if let Some(p) = m.get("permissions") {
        let allow_nonempty = |key: &str| {
            p.get(key)
                .and_then(|n| n.get("allow"))
                .and_then(|a| a.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false)
        };
        if allow_nonempty("network") {
            granted.push("network");
        }
        if allow_nonempty("subprocess") {
            granted.push("subprocess");
        }
        if p.get("secrets").and_then(|v| v.as_bool()).unwrap_or(false) {
            granted.push("secrets");
        }
        if p.get("sendText").and_then(|v| v.as_bool()).unwrap_or(false) {
            granted.push("send_text");
        }
        if p.get("agentCommands")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            granted.push("agent_commands");
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
        // Documentation surface for the detail pane. `description`/`homepage` are
        // plain manifest strings; `readme` is a relative markdown path (default
        // README.md) loaded into a bounded `readme_markdown` field so the UI can
        // render docs without a second round-trip.
        if let Some(desc) = m.get("description").and_then(|v| v.as_str()) {
            obj.insert("description".into(), serde_json::json!(desc));
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

fn build_snapshot(last_output: Option<Value>) -> Result<Value, String> {
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

fn plugin_action_args(action: &PluginManagerAction) -> Result<Vec<String>, String> {
    let mut args = vec!["plugin".to_string()];
    match action.kind.as_str() {
        "refresh-channel" => {
            args.extend(["channel".into(), "refresh".into()]);
            if let Some(channel) = &action.channel {
                args.push(channel.clone());
            }
        }
        "add-channel" => {
            let url = action.url.clone().ok_or("add-channel requires url")?;
            args.extend(["channel".into(), "add".into(), url]);
            if let Some(name) = &action.name {
                args.extend(["--name".into(), name.clone()]);
            }
        }
        "remove-channel" => {
            let channel = action
                .channel
                .clone()
                .ok_or("remove-channel requires channel")?;
            if is_protected_channel(&channel) {
                return Err(format!("channel '{channel}' is protected"));
            }
            args.extend(["channel".into(), "remove".into(), channel]);
            if action.force {
                args.push("--force".into());
            }
        }
        "install" => {
            let plugin_id = action
                .plugin_id
                .clone()
                .ok_or("install requires pluginId")?;
            args.extend(["install".into(), plugin_id]);
            if let Some(channel) = &action.channel {
                args.extend(["--channel".into(), channel.clone()]);
            }
        }
        "update" => {
            args.push("update".into());
            if let Some(plugin_id) = &action.plugin_id {
                args.push(plugin_id.clone());
            }
        }
        "update-system" => {
            args.push("update-system".into());
            if let Some(plugin_id) = &action.plugin_id {
                args.push(plugin_id.clone());
            }
        }
        "remove" => {
            let plugin_id = action.plugin_id.clone().ok_or("remove requires pluginId")?;
            args.extend(["remove".into(), plugin_id]);
            if action.force {
                args.push("--force".into());
            }
        }
        other => return Err(format!("unknown plugin manager action: {other}")),
    }
    args.push("--json".into());
    Ok(args)
}

fn run_plugin_cli(args: &[impl AsRef<str>]) -> Result<Value, String> {
    let data_dir = crate::paths::data_dir();
    let cli = cli_path(&data_dir);
    let output = Command::new(&cli)
        .args(args.iter().map(|a| a.as_ref()))
        .env("CODETERM_DATA_DIR", &data_dir)
        .env("CODETERM_VERSION", env!("CARGO_PKG_VERSION"))
        .output()
        .map_err(|e| format!("run {}: {e}", cli.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("plugin command failed with status {}", output.status)
        } else {
            stderr
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim()).map_err(|e| format!("parse plugin CLI JSON: {e}"))
}

fn cli_path(data_dir: &std::path::Path) -> std::path::PathBuf {
    let bin = if cfg!(windows) {
        "codeterm.exe"
    } else {
        "codeterm"
    };
    data_dir.join("bin").join(bin)
}

fn read_audit_events(limit: usize) -> Vec<Value> {
    let path = crate::plugin::audit::log_path(&crate::paths::data_dir());
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut events: Vec<Value> = text
        .lines()
        .rev()
        .take(limit)
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect();
    events.reverse();
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_action_builds_json_cli_args() {
        let action = PluginManagerAction {
            kind: "update".into(),
            plugin_id: Some("demo".into()),
            channel: None,
            url: None,
            name: None,
            force: false,
        };
        assert_eq!(
            plugin_action_args(&action).unwrap(),
            vec!["plugin", "update", "demo", "--json"]
        );
    }

    #[test]
    fn add_channel_requires_url() {
        let action = PluginManagerAction {
            kind: "add-channel".into(),
            plugin_id: None,
            channel: None,
            url: None,
            name: Some("demo".into()),
            force: false,
        };
        assert!(plugin_action_args(&action).unwrap_err().contains("url"));
    }

    #[test]
    fn remove_action_builds_json_cli_args_with_optional_force() {
        let base = PluginManagerAction {
            kind: "remove".into(),
            plugin_id: Some("demo".into()),
            channel: None,
            url: None,
            name: None,
            force: false,
        };
        assert_eq!(
            plugin_action_args(&base).unwrap(),
            vec!["plugin", "remove", "demo", "--json"]
        );
        let forced = PluginManagerAction {
            force: true,
            ..base
        };
        assert_eq!(
            plugin_action_args(&forced).unwrap(),
            vec!["plugin", "remove", "demo", "--force", "--json"]
        );
    }

    #[test]
    fn remove_action_requires_plugin_id() {
        let action = PluginManagerAction {
            kind: "remove".into(),
            plugin_id: None,
            channel: None,
            url: None,
            name: None,
            force: false,
        };
        assert!(plugin_action_args(&action)
            .unwrap_err()
            .contains("pluginId"));
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
