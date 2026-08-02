use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

use crate::service::ServiceContext;

use super::cli::run_plugin_cli;
use super::snapshot::build_snapshot;

const PROTECTED_PLUGIN_CHANNELS: [&str; 2] = ["codeterm-core", "codeterm-plugins"];

type SnapshotRebuild = Arc<dyn Fn() -> Result<Value, String> + Send + Sync + 'static>;

#[derive(Clone)]
struct SnapshotFlight {
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
    if let Ok(mut flight) = SNAPSHOT_FLIGHT.lock() {
        *flight = None;
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
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub remove_data: bool,
}

pub(super) async fn plugin_manager_snapshot_impl(
    svc: tauri::State<'_, crate::service::ServiceState>,
) -> Result<Value, String> {
    plugin_manager_snapshot_for_service(svc.inner().clone()).await
}

pub(crate) async fn plugin_manager_snapshot_for_service(
    svc: crate::service::ServiceState,
) -> Result<Value, String> {
    cached_plugin_snapshot(Arc::new(move || {
        build_snapshot(None, Some(svc.chatbackend_registry.as_ref()))
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

    let role = snapshot_flight_role();
    match role {
        SnapshotFlightRole::Leader(flight, epoch) => {
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
            let _ = flight.finish.send(Some(result.clone()));
            result
        }
        SnapshotFlightRole::Follower(mut flight) => loop {
            let result = { flight.done.borrow().as_ref().cloned() };
            if let Some(result) = result {
                return result.clone();
            }
            flight.done.changed().await.map_err(|e| e.to_string())?;
        },
    }
}

fn snapshot_flight_role() -> SnapshotFlightRole {
    if let Ok(mut in_progress) = SNAPSHOT_FLIGHT.lock() {
        if let Some(flight) = in_progress.as_ref() {
            return SnapshotFlightRole::Follower(flight.clone());
        }
        let (finish, done) = tokio::sync::watch::channel(None);
        let flight = SnapshotFlight { done, finish };
        *in_progress = Some(flight.clone());
        return SnapshotFlightRole::Leader(
            flight,
            SNAPSHOT_EPOCH.load(std::sync::atomic::Ordering::SeqCst),
        );
    }

    let (finish, done) = tokio::sync::watch::channel(None);
    SnapshotFlightRole::Leader(
        SnapshotFlight { done, finish },
        SNAPSHOT_EPOCH.load(std::sync::atomic::Ordering::SeqCst),
    )
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
        // Action responses are authoritative UI state. Recompute before snapshotting so
        // install/update/remove and channel actions cannot return their pre-action drift.
        if action_refreshes_catalog(&action.kind) {
            super::snapshot::refresh_channel_diff_now();
        }
        build_snapshot(Some(output), Some(svc.chatbackend_registry.as_ref()))
    })();

    let finished = svc
        .plugin_activity
        .finish(&activity.id, result.as_ref().err().cloned());
    crate::plugin::activity::emit_plugin_activity(svc, &finished);
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
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn counted_rebuild(count: Arc<AtomicUsize>, delay: std::time::Duration) -> SnapshotRebuild {
        Arc::new(move || {
            let n = count.fetch_add(1, Ordering::SeqCst) + 1;
            if !delay.is_zero() {
                std::thread::sleep(delay);
            }
            Ok(json!({ "generation": n }))
        })
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn cold_concurrent_snapshots_share_one_rebuild() {
        let _guard = TEST_LOCK.lock().await;
        invalidate_plugin_snapshot();
        let count = Arc::new(AtomicUsize::new(0));
        let rebuild = counted_rebuild(count.clone(), std::time::Duration::from_millis(75));
        let mut handles = Vec::new();

        for _ in 0..12 {
            let rebuild = rebuild.clone();
            handles.push(tokio::spawn(async move {
                cached_plugin_snapshot(rebuild).await.unwrap()
            }));
        }

        let mut results = Vec::new();
        for handle in handles {
            results.push(handle.await.unwrap());
        }

        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert!(results.iter().all(|v| v == &json!({ "generation": 1 })));
        invalidate_plugin_snapshot();
    }

    #[tokio::test]
    async fn snapshot_cache_hit_does_not_rebuild() {
        let _guard = TEST_LOCK.lock().await;
        invalidate_plugin_snapshot();
        let count = Arc::new(AtomicUsize::new(0));
        let rebuild = counted_rebuild(count.clone(), std::time::Duration::ZERO);

        let first = cached_plugin_snapshot(rebuild.clone()).await.unwrap();
        let second = cached_plugin_snapshot(rebuild).await.unwrap();

        assert_eq!(first, json!({ "generation": 1 }));
        assert_eq!(second, first);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        invalidate_plugin_snapshot();
    }

    #[tokio::test]
    async fn snapshot_invalidation_forces_rebuild() {
        let _guard = TEST_LOCK.lock().await;
        invalidate_plugin_snapshot();
        let count = Arc::new(AtomicUsize::new(0));
        let rebuild = counted_rebuild(count.clone(), std::time::Duration::ZERO);

        let first = cached_plugin_snapshot(rebuild.clone()).await.unwrap();
        invalidate_plugin_snapshot();
        let second = cached_plugin_snapshot(rebuild).await.unwrap();

        assert_eq!(first, json!({ "generation": 1 }));
        assert_eq!(second, json!({ "generation": 2 }));
        assert_eq!(count.load(Ordering::SeqCst), 2);
        invalidate_plugin_snapshot();
    }

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
    fn every_supported_action_refreshes_the_returned_catalog() {
        for kind in [
            "install",
            "update",
            "update-system",
            "remove",
            "refresh-channel",
            "add-channel",
            "remove-channel",
        ] {
            assert!(
                action_refreshes_catalog(kind),
                "{kind} returned stale drift"
            );
        }
        assert!(!action_refreshes_catalog("unknown"));
    }

    #[test]
    fn update_action_builds_json_cli_args_without_plugin_id() {
        let action = PluginManagerAction {
            kind: "update".into(),
            plugin_id: None,
            channel: None,
            url: None,
            name: None,
            force: false,
            remove_data: false,
        };
        assert_eq!(
            plugin_action_args(&action).unwrap(),
            vec!["plugin", "update", "--json"]
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
