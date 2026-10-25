use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

use codeterm_service_core::service::ServiceContext;

use super::cli::run_plugin_cli;
use super::snapshot::build_snapshot;

const PROTECTED_PLUGIN_CHANNELS: [&str; 2] = ["codeterm-core", "codeterm-plugins"];

type SnapshotRebuild = Arc<dyn Fn() -> Result<Value, String> + Send + Sync + 'static>;

#[derive(Clone)]
struct SnapshotFlight {
    epoch: u64,
    done: tokio::sync::watch::Receiver<Option<Result<Value, String>>>,
    finish: tokio::sync::watch::Sender<Option<Result<Value, String>>>,
}

enum SnapshotFlightRole {
    Leader(SnapshotFlight, u64),
    Follower(SnapshotFlight),
}

static SNAPSHOT_CACHE: std::sync::Mutex<Option<(std::time::Instant, Value)>> =
    std::sync::Mutex::new(None);
static SNAPSHOT_FLIGHT: std::sync::Mutex<Option<SnapshotFlight>> = std::sync::Mutex::new(None);
static SNAPSHOT_EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
const SNAPSHOT_TTL: std::time::Duration = std::time::Duration::from_secs(15);

pub(crate) fn invalidate_plugin_snapshot() {
    if let Ok(mut c) = SNAPSHOT_CACHE.lock() {
        *c = None;
    }
    SNAPSHOT_EPOCH.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    // while the last-known diff stays visible.
    super::diff_cache::invalidate_diff();
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManagerAction {
    pub kind: String,
    pub plugin_id: Option<String>,
    pub channel: Option<String>,
    pub url: Option<String>,
    pub name: Option<String>,
    pub permission: Option<String>,
    #[serde(default)]
    pub permission_source: codeterm_plugin::plugin_lifecycle::PermissionChangeSource,
    #[serde(default)]
    pub allowed: bool,
    #[serde(default)]
    pub allow_hosts: Vec<String>,
    #[serde(default)]
    pub disallow_hosts: Vec<String>,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub remove_data: bool,
}

pub(crate) async fn plugin_manager_snapshot_for_service(
    svc: codeterm_service_core::service::ServiceState,
    lifetime: impl Send + Sync + 'static,
) -> Result<Value, String> {
    cached_plugin_snapshot_with_lifetime(
        Arc::new(move || build_snapshot(None, Some(svc.chatbackend_registry.as_ref()))),
        lifetime,
    )
    .await
}

async fn cached_plugin_snapshot_with_lifetime(
    rebuild: SnapshotRebuild,
    lifetime: impl Send + Sync + 'static,
) -> Result<Value, String> {
    cached_plugin_snapshot(Arc::new(move || {
        let _lifetime = &lifetime;
        rebuild()
    }))
    .await
}

async fn cached_plugin_snapshot(rebuild: SnapshotRebuild) -> Result<Value, String> {
    if let Ok(guard) = SNAPSHOT_CACHE.lock() {
        if let Some((at, v)) = guard.as_ref() {
            if at.elapsed() < SNAPSHOT_TTL {
                return Ok(v.clone());
            }
        }
    }

    let (mut done, epoch) = match snapshot_flight_role() {
        SnapshotFlightRole::Leader(flight, epoch) => {
            let done = flight.done.clone();
            // Detached so a cancelled HTTP request cannot strand the shared flight and every later caller.
            tokio::spawn(finish_snapshot_flight(flight, epoch, rebuild));
            (done, epoch)
        }
        SnapshotFlightRole::Follower(flight) => (flight.done, flight.epoch),
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(result) = done.borrow().as_ref().cloned() {
                if SNAPSHOT_EPOCH.load(std::sync::atomic::Ordering::SeqCst) != epoch {
                    return Err("plugin_unavailable: snapshot changed during rebuild".to_string());
                }
                return result;
            }
            done.changed().await.map_err(|e| e.to_string())?;
        }
    })
    .await
    .map_err(|_| "plugin_unavailable: snapshot rebuild exceeded its deadline".to_string())?
}

async fn finish_snapshot_flight(flight: SnapshotFlight, epoch: u64, rebuild: SnapshotRebuild) {
    let result = tokio::task::spawn_blocking(move || rebuild())
        .await
        .map_err(|e| e.to_string())
        .and_then(|v| v);
    if let Ok(mut in_progress) = SNAPSHOT_FLIGHT.lock() {
        if in_progress
            .as_ref()
            .is_some_and(|current| current.finish.same_channel(&flight.finish))
        {
            *in_progress = None;
        }
    }
    if let Ok(snap) = result.as_ref() {
        if SNAPSHOT_EPOCH.load(std::sync::atomic::Ordering::SeqCst) == epoch {
            if let Ok(mut guard) = SNAPSHOT_CACHE.lock() {
                *guard = Some((std::time::Instant::now(), snap.clone()));
            }
        }
    }
    let _ = flight.finish.send(Some(result));
}

fn snapshot_flight_role() -> SnapshotFlightRole {
    if let Ok(mut in_progress) = SNAPSHOT_FLIGHT.lock() {
        if let Some(flight) = in_progress.as_ref() {
            return SnapshotFlightRole::Follower(flight.clone());
        }
        let (finish, done) = tokio::sync::watch::channel(None);
        let flight = SnapshotFlight {
            done,
            finish,
            epoch: SNAPSHOT_EPOCH.load(std::sync::atomic::Ordering::SeqCst),
        };
        *in_progress = Some(flight.clone());
        return SnapshotFlightRole::Leader(
            flight,
            SNAPSHOT_EPOCH.load(std::sync::atomic::Ordering::SeqCst),
        );
    }

    let (finish, done) = tokio::sync::watch::channel(None);
    SnapshotFlightRole::Leader(
        SnapshotFlight {
            done,
            finish,
            epoch: SNAPSHOT_EPOCH.load(std::sync::atomic::Ordering::SeqCst),
        },
        SNAPSHOT_EPOCH.load(std::sync::atomic::Ordering::SeqCst),
    )
}

pub fn run_action(action: PluginManagerAction, svc: &ServiceContext) -> Result<Value, String> {
    if action.kind == "permission" {
        let output = super::permissions::permission_action(
            action
                .plugin_id
                .as_deref()
                .ok_or("permission requires pluginId")?,
            action
                .permission
                .as_deref()
                .ok_or("permission requires permission")?,
            action.allowed,
            action.permission_source,
        )?;
        return build_snapshot(Some(output), Some(svc.chatbackend_registry.as_ref()));
    }
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
    let activity = svc
        .ext::<Arc<codeterm_service_core::plugin::activity::PluginActivityRegistry>>()
        .start(
            &activity_meta.plugin_id,
            activity_meta.kind,
            &activity_meta.label,
        );
    codeterm_service_core::plugin::activity::emit_plugin_activity(svc, &activity);

    let result = (|| {
        if action.kind == "remove-channel" {
            let channel = action
                .channel
                .as_deref()
                .ok_or("remove-channel requires channel")?;
            remove_channel_plugins(channel, action.force)?;
        }
        let output = run_plugin_cli(&args)?;
        if action.kind == "refresh-channel"
            && action
                .channel
                .as_deref()
                .is_none_or(|c| c == "codeterm-core")
        {
            if let Err(error) = super::cli::with_install_lifecycle(|| {
                codeterm_plugins::sync_core_cached(codeterm_plugins::PluginCtx {
                    data_root: codeterm_service_core::paths::data_dir(),
                    version: codeterm_build_info::installed_version().map(str::to_string),
                })
            }) {
                log::warn!("[plugin-update] silent core sync failed: {error}");
            }
        }
        if action.kind == "install" {
            svc.record_product_feature(codeterm_analytics::Feature::PluginInstalled);
        }
        if action.kind == "remove" {
            let plugin_id = action
                .plugin_id
                .as_deref()
                .ok_or("remove requires pluginId")?;
            codeterm_service_core::config::cleanup_removed_plugin_config(plugin_id)?;
        }
        if action_refreshes_catalog(&action.kind) {
            codeterm_service_core::service::plugin_skills::sync(&svc.mem);
        }
        // Action responses are authoritative UI state. Recompute before snapshotting so
        // install/update/remove and channel actions cannot return their pre-action drift.
        if action_refreshes_catalog(&action.kind) {
            super::snapshot::refresh_channel_diff_now();
        }
        let snapshot = build_snapshot(Some(output), Some(svc.chatbackend_registry.as_ref()))?;
        crate::update::discovery::publish_plugin_snapshot(&svc.ui, &snapshot);
        Ok(snapshot)
    })();

    let finished = svc
        .ext::<Arc<codeterm_service_core::plugin::activity::PluginActivityRegistry>>()
        .finish(&activity.id, result.as_ref().err().cloned());
    codeterm_service_core::plugin::activity::emit_plugin_activity(svc, &finished);
    result
}

fn action_refreshes_catalog(kind: &str) -> bool {
    matches!(
        kind,
        "install"
            | "update"
            | "update-system"
            | "remove"
            | "refresh-channel"
            | "add-channel"
            | "remove-channel"
    )
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
        "settings" => {
            let id = action
                .plugin_id
                .as_deref()
                .ok_or("settings requires pluginId")?;
            Ok(PluginActionActivity {
                plugin_id: id.to_string(),
                kind: "settings",
                label: format!("Updating {id} network grants…"),
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
        "settings" => {
            let plugin_id = action
                .plugin_id
                .clone()
                .ok_or("settings requires pluginId")?;
            args.extend(["settings".into(), plugin_id]);
            for host in &action.allow_hosts {
                args.extend(["--allow-host".into(), host.clone()]);
            }
            for host in &action.disallow_hosts {
                args.extend(["--disallow-host".into(), host.clone()]);
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
mod tests;
