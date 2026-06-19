use serde::Deserialize;
use serde_json::Value;

use crate::service::ServiceContext;

use super::cli::run_plugin_cli;
use super::snapshot::build_snapshot;

const PROTECTED_PLUGIN_CHANNELS: [&str; 2] = ["codeterm-core", "codeterm-plugins"];

static SNAPSHOT_CACHE: std::sync::Mutex<Option<(std::time::Instant, Value)>> =
    std::sync::Mutex::new(None);
const SNAPSHOT_TTL: std::time::Duration = std::time::Duration::from_secs(15);

pub(crate) fn invalidate_plugin_snapshot() {
    if let Ok(mut c) = SNAPSHOT_CACHE.lock() {
        *c = None;
    }
}

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
    #[serde(default)]
    pub remove_data: bool,
}

pub(super) async fn plugin_manager_snapshot_impl(
    svc: tauri::State<'_, crate::service::ServiceState>,
) -> Result<Value, String> {
    if let Ok(guard) = SNAPSHOT_CACHE.lock() {
        if let Some((at, v)) = guard.as_ref() {
            if at.elapsed() < SNAPSHOT_TTL {
                return Ok(v.clone());
            }
        }
    }
    let svc = svc.inner().clone();
    let snap = tokio::task::spawn_blocking(move || {
        build_snapshot(None, Some(svc.chatbackend_registry.as_ref()))
    })
    .await
    .map_err(|e| e.to_string())??;
    if let Ok(mut guard) = SNAPSHOT_CACHE.lock() {
        *guard = Some((std::time::Instant::now(), snap.clone()));
    }
    Ok(snap)
}

pub(super) async fn plugin_manager_action_impl(
    svc: tauri::State<'_, crate::service::ServiceState>,
    action: PluginManagerAction,
) -> Result<Value, String> {
    let svc = svc.inner().clone();
    let out = tokio::task::spawn_blocking(move || run_action(action, svc.as_ref()))
        .await
        .map_err(|e| e.to_string())?;
    invalidate_plugin_snapshot();
    out
}

pub(crate) fn run_action(
    action: PluginManagerAction,
    svc: &ServiceContext,
) -> Result<Value, String> {
    if action.kind == "remove-channel" {
        let channel = action
            .channel
            .clone()
            .ok_or("remove-channel requires channel")?;
        if is_protected_channel(&channel) {
            return Err(format!("channel '{channel}' is protected"));
        }
    }
    let args = plugin_action_args(&action)?;
    let activity_meta = plugin_action_activity(&action)?;
    let activity = svc.plugin_activity.start(
        &activity_meta.plugin_id,
        activity_meta.kind,
        &activity_meta.label,
    );
    crate::plugin::activity::emit_plugin_activity(svc, &activity);

    let result = (|| {
        if action.kind == "remove-channel" {
            let channel = action
                .channel
                .as_deref()
                .ok_or("remove-channel requires channel")?;
            remove_channel_plugins(channel, action.force)?;
        }
        let output = run_plugin_cli(&args)?;
        if action.kind == "remove" {
            let plugin_id = action
                .plugin_id
                .as_deref()
                .ok_or("remove requires pluginId")?;
            crate::config::cleanup_removed_plugin_config(plugin_id)?;
        }
        build_snapshot(Some(output), Some(svc.chatbackend_registry.as_ref()))
    })();

    let finished = svc
        .plugin_activity
        .finish(&activity.id, result.as_ref().err().cloned());
    crate::plugin::activity::emit_plugin_activity(svc, &finished);
    result
}

struct PluginActionActivity {
    plugin_id: String,
    kind: &'static str,
    label: String,
}

fn plugin_action_activity(action: &PluginManagerAction) -> Result<PluginActionActivity, String> {
    match action.kind.as_str() {
        "install" => {
            let id = action
                .plugin_id
                .as_deref()
                .ok_or("install requires pluginId")?;
            Ok(PluginActionActivity {
                plugin_id: id.to_string(),
                kind: "install",
                label: format!("Installing {id}…"),
            })
        }
        "remove" => {
            let id = action
                .plugin_id
                .as_deref()
                .ok_or("remove requires pluginId")?;
            Ok(PluginActionActivity {
                plugin_id: id.to_string(),
                kind: "uninstall",
                label: format!("Uninstalling {id}…"),
            })
        }
        "update" | "update-system" => {
            let id = action.plugin_id.as_deref().unwrap_or("plugins");
            Ok(PluginActionActivity {
                plugin_id: id.to_string(),
                kind: "update",
                label: format!("Updating {id}…"),
            })
        }
        "refresh-channel" => {
            let id = action.channel.as_deref().unwrap_or("channels");
            Ok(PluginActionActivity {
                plugin_id: format!("channel:{id}"),
                kind: "channel",
                label: format!("Refreshing {id}…"),
            })
        }
        "add-channel" => {
            let id = action
                .name
                .as_deref()
                .or(action.url.as_deref())
                .ok_or("add-channel requires url")?;
            Ok(PluginActionActivity {
                plugin_id: format!("channel:{id}"),
                kind: "channel",
                label: format!("Adding channel {id}…"),
            })
        }
        "remove-channel" => {
            let id = action
                .channel
                .as_deref()
                .ok_or("remove-channel requires channel")?;
            Ok(PluginActionActivity {
                plugin_id: format!("channel:{id}"),
                kind: "channel",
                label: format!("Removing channel {id}…"),
            })
        }
        other => Err(format!("unknown plugin manager action: {other}")),
    }
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
            if action.remove_data {
                args.push("--remove-data".into());
            }
        }
        other => return Err(format!("unknown plugin manager action: {other}")),
    }
    args.push("--json".into());
    Ok(args)
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
            remove_data: false,
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
            remove_data: false,
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
            remove_data: false,
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
        let remove_data = PluginManagerAction {
            remove_data: true,
            ..forced
        };
        assert_eq!(
            plugin_action_args(&remove_data).unwrap(),
            vec![
                "plugin",
                "remove",
                "demo",
                "--force",
                "--remove-data",
                "--json"
            ]
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
            remove_data: false,
        };
        assert!(plugin_action_args(&action)
            .unwrap_err()
            .contains("pluginId"));
    }

    #[test]
    fn plugin_action_activity_maps_install_remove_and_update() {
        let install = PluginManagerAction {
            kind: "install".into(),
            plugin_id: Some("transcriber".into()),
            channel: None,
            url: None,
            name: None,
            force: false,
            remove_data: false,
        };
        let install_activity = plugin_action_activity(&install).unwrap();
        assert_eq!(install_activity.plugin_id, "transcriber");
        assert_eq!(install_activity.kind, "install");
        assert_eq!(install_activity.label, "Installing transcriber…");

        let remove = PluginManagerAction {
            kind: "remove".into(),
            plugin_id: Some("transcriber".into()),
            channel: None,
            url: None,
            name: None,
            force: false,
            remove_data: false,
        };
        let remove_activity = plugin_action_activity(&remove).unwrap();
        assert_eq!(remove_activity.plugin_id, "transcriber");
        assert_eq!(remove_activity.kind, "uninstall");
        assert_eq!(remove_activity.label, "Uninstalling transcriber…");

        let update_system = PluginManagerAction {
            kind: "update-system".into(),
            plugin_id: Some("codeterm-core".into()),
            channel: None,
            url: None,
            name: None,
            force: false,
            remove_data: false,
        };
        let update_activity = plugin_action_activity(&update_system).unwrap();
        assert_eq!(update_activity.plugin_id, "codeterm-core");
        assert_eq!(update_activity.kind, "update");
        assert_eq!(update_activity.label, "Updating codeterm-core…");
    }

    #[test]
    fn plugin_action_activity_maps_channel_operations() {
        let add = PluginManagerAction {
            kind: "add-channel".into(),
            plugin_id: None,
            channel: None,
            url: Some("https://example.test/channel.json".into()),
            name: Some("demo".into()),
            force: false,
            remove_data: false,
        };
        let add_activity = plugin_action_activity(&add).unwrap();
        assert_eq!(add_activity.plugin_id, "channel:demo");
        assert_eq!(add_activity.kind, "channel");
        assert_eq!(add_activity.label, "Adding channel demo…");

        let refresh = PluginManagerAction {
            kind: "refresh-channel".into(),
            plugin_id: None,
            channel: Some("demo".into()),
            url: None,
            name: None,
            force: false,
            remove_data: false,
        };
        let refresh_activity = plugin_action_activity(&refresh).unwrap();
        assert_eq!(refresh_activity.plugin_id, "channel:demo");
        assert_eq!(refresh_activity.kind, "channel");
        assert_eq!(refresh_activity.label, "Refreshing demo…");
    }
}
