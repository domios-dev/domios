use std::process::Command;

use serde::Deserialize;
use serde_json::Value;

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

/// Run a plugin-manager action and return the refreshed snapshot. Shared by the
/// Tauri command and the web-mirror REST route (`POST /api/plugins/action`).
pub(crate) fn run_action(action: PluginManagerAction) -> Result<Value, String> {
    let args = plugin_action_args(&action)?;
    let output = run_plugin_cli(&args)?;
    build_snapshot(Some(output))
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
        Some(c) => c.get("agentProvider").and_then(|v| v.as_bool()).unwrap_or(false),
        None => true,
    };
    let cap_obj = serde_json::json!({
        "agent_provider": agent_provider,
        "secret_backend": caps.and_then(|c| c.get("secretBackend")).and_then(|v| v.as_bool()).unwrap_or(false),
        "transcriber": caps.and_then(|c| c.get("transcriber")).and_then(|v| v.as_bool()).unwrap_or(false),
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
    }
    let settings_schema = m
        .get("settingsSchema")
        .and_then(|v| v.as_str())
        .and_then(|rel| std::fs::read_to_string(dir.join(rel)).ok())
        .and_then(|t| serde_json::from_str::<Value>(&t).ok());

    if let Some(obj) = entry.as_object_mut() {
        obj.insert("capabilities".into(), cap_obj);
        obj.insert("granted_permissions".into(), serde_json::json!(granted));
        if let Some(schema) = settings_schema {
            obj.insert("settings_schema".into(), schema);
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
    }
}

fn build_snapshot(last_output: Option<Value>) -> Result<Value, String> {
    let mut plugins = run_plugin_cli(&["plugin", "list", "--json"])?;
    let manifests = scan_plugin_manifests();
    if let Some(arr) = plugins.get_mut("plugins").and_then(|p| p.as_array_mut()) {
        let mut seen = std::collections::HashSet::new();
        for entry in arr.iter_mut() {
            if let Some(id) = entry.get("id").and_then(|v| v.as_str()).map(String::from) {
                seen.insert(id.clone());
                if let Some((dir, m)) = manifests.get(&id) {
                    enrich_from_manifest(entry, dir, m);
                }
            }
        }
        // Surface plugins the daemon loaded from disk but the CLI didn't list
        // (e.g. workspace capability plugins in dev) so the Extensions view is honest.
        for (id, (dir, m)) in &manifests {
            if seen.contains(id) {
                continue;
            }
            let mut entry = serde_json::json!({
                "id": id,
                "state": "active",
                "system": false,
                "removable": true,
                "install_path": dir.to_string_lossy(),
            });
            enrich_from_manifest(&mut entry, dir, m);
            arr.push(entry);
        }
    }
    let channels = run_plugin_cli(&["plugin", "channel", "list", "--json"])?;
    let diff = run_plugin_cli(&["plugin", "channel", "diff", "--json"]).unwrap_or_else(|e| {
        serde_json::json!({
            "diff": [],
            "error": e,
        })
    });
    let audit = read_audit_events(80);
    Ok(serde_json::json!({
        "plugins": plugins.get("plugins").cloned().unwrap_or_else(|| serde_json::json!([])),
        "channels": channels.get("channels").cloned().unwrap_or_else(|| serde_json::json!([])),
        "diff": diff.get("diff").cloned().unwrap_or_else(|| serde_json::json!([])),
        "audit": audit,
        "lastOutput": last_output,
    }))
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
}
