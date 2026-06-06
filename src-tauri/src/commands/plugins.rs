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
    let args = plugin_action_args(&action)?;
    let output = run_plugin_cli(&args)?;
    build_snapshot(Some(output))
}

fn build_snapshot(last_output: Option<Value>) -> Result<Value, String> {
    let plugins = run_plugin_cli(&["plugin", "list", "--json"])?;
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
