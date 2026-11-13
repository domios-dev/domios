use super::*;

#[test]
fn directory_install_request_enters_lifecycle_after_host_compatibility_gate() {
    struct Lifecycle(std::sync::atomic::AtomicUsize);
    impl codeterm_plugins::InstallLifecycle for Lifecycle {
        fn install(
            &self,
            _: &str,
            _: &std::path::Path,
            operation: &mut codeterm_plugins::InstallOperation<'_>,
        ) -> Result<(), String> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            operation(&mut || Ok(()))
        }
    }
    for (version, minimum, compatible) in [
        (Some("1.19.2"), "1.19.2", true),
        (Some("1.19.2"), "1.20.0", false),
        (None, "1.19.2", false),
    ] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("plugin.json"), serde_json::json!({
            "id":"fixture", "version":"1.0.0", "displayName":"Fixture", "entry":"plugin.js", "minCodeterm":minimum
        }).to_string()).unwrap();
        std::fs::write(source.join("plugin.js"), "module.exports = {};").unwrap();
        let action: PluginManagerAction = serde_json::from_value(serde_json::json!({
            "kind":"install", "fromDir":source, "idOverride":"renamed"
        }))
        .unwrap();
        assert_eq!(
            plugin_action_activity(&action).unwrap().plugin_id,
            "renamed"
        );
        let args = plugin_action_args(&action).unwrap();
        let lifecycle = Arc::new(Lifecycle(std::sync::atomic::AtomicUsize::new(0)));
        let data = root.path().join("data");
        let result = super::super::cli::run_with_lifecycle(
            &args,
            codeterm_plugins::PluginCtx {
                data_root: data.clone(),
                version: version.map(str::to_owned),
            },
            lifecycle.clone(),
        );
        if compatible {
            assert_eq!(result.unwrap()["id"], "renamed");
        } else {
            assert!(result.unwrap_err().contains("requires Domios"));
        }
        assert_eq!(
            lifecycle.0.load(std::sync::atomic::Ordering::SeqCst),
            usize::from(compatible)
        );
        assert_eq!(
            data.join("plugins/renamed/plugin.json").exists(),
            compatible
        );
    }
}
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test(start_paused = true)]
async fn stuck_snapshot_rebuild_times_out_without_releasing_its_worker_slot() {
    let _guard = TEST_LOCK.lock().await;
    invalidate_plugin_snapshot();
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    let started = std::sync::Mutex::new(Some(started));
    let blocked = std::sync::Mutex::new(blocked);
    let rebuild: SnapshotRebuild = Arc::new(move || {
        started.lock().unwrap().take().unwrap().send(()).unwrap();
        blocked.lock().unwrap().recv().unwrap();
        Ok(json!({ "released": true }))
    });
    struct Lifetime(Option<tokio::sync::oneshot::Sender<()>>);
    impl Drop for Lifetime {
        fn drop(&mut self) {
            let _ = self.0.take().unwrap().send(());
        }
    }
    let (dropped, mut lifetime_ended) = tokio::sync::oneshot::channel();
    let request = tokio::spawn(cached_plugin_snapshot_with_lifetime(
        rebuild,
        Lifetime(Some(dropped)),
    ));
    ready.await.unwrap();
    tokio::time::advance(std::time::Duration::from_secs(5)).await;
    let result = request.await.unwrap();
    let flight = SNAPSHOT_FLIGHT.lock().unwrap().clone();
    invalidate_plugin_snapshot();
    let retained = SNAPSHOT_FLIGHT.lock().unwrap().clone();
    assert!(matches!(
        lifetime_ended.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));
    release.send(()).unwrap();
    lifetime_ended.await.unwrap();
    let mut done = flight.as_ref().unwrap().done.clone();
    done.wait_for(Option::is_some).await.unwrap();
    assert!(result.unwrap_err().contains("plugin_unavailable"));
    assert!(retained
        .unwrap()
        .finish
        .same_channel(&flight.unwrap().finish));
    invalidate_plugin_snapshot();
}

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
        from_dir: None,
        id_override: None,
        kind: "update".into(),
        plugin_id: Some("demo".into()),
        channel: None,
        url: None,
        name: None,
        permission: None,
        permission_source: Default::default(),
        allowed: false,
        force: false,
        remove_data: false,
        allow_hosts: Vec::new(),
        disallow_hosts: Vec::new(),
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
        from_dir: None,
        id_override: None,
        kind: "update".into(),
        plugin_id: None,
        channel: None,
        url: None,
        name: None,
        permission: None,
        permission_source: Default::default(),
        allowed: false,
        force: false,
        remove_data: false,
        allow_hosts: Vec::new(),
        disallow_hosts: Vec::new(),
    };
    assert_eq!(
        plugin_action_args(&action).unwrap(),
        vec!["plugin", "update", "--json"]
    );
}

#[test]
fn settings_action_builds_shared_cli_args() {
    let action = PluginManagerAction {
        from_dir: None,
        id_override: None,
        kind: "settings".into(),
        plugin_id: Some("demo".into()),
        channel: None,
        url: None,
        name: None,
        permission: None,
        permission_source: Default::default(),
        allowed: false,
        force: false,
        remove_data: false,
        allow_hosts: vec!["api.example.test".into()],
        disallow_hosts: vec!["old.example.test:8443".into()],
    };
    assert_eq!(
        plugin_action_args(&action).unwrap(),
        vec![
            "plugin",
            "settings",
            "demo",
            "--allow-host",
            "api.example.test",
            "--disallow-host",
            "old.example.test:8443",
            "--json",
        ]
    );
}

#[test]
fn add_channel_requires_url() {
    let action = PluginManagerAction {
        from_dir: None,
        id_override: None,
        kind: "add-channel".into(),
        plugin_id: None,
        channel: None,
        url: None,
        name: Some("demo".into()),
        permission: None,
        permission_source: Default::default(),
        allowed: false,
        force: false,
        remove_data: false,
        allow_hosts: Vec::new(),
        disallow_hosts: Vec::new(),
    };
    assert!(plugin_action_args(&action).unwrap_err().contains("url"));
}

#[test]
fn remove_action_builds_json_cli_args_with_optional_force() {
    let base = PluginManagerAction {
        from_dir: None,
        id_override: None,
        kind: "remove".into(),
        plugin_id: Some("demo".into()),
        channel: None,
        url: None,
        name: None,
        permission: None,
        permission_source: Default::default(),
        allowed: false,
        force: false,
        remove_data: false,
        allow_hosts: Vec::new(),
        disallow_hosts: Vec::new(),
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
        from_dir: None,
        id_override: None,
        kind: "remove".into(),
        plugin_id: None,
        channel: None,
        url: None,
        name: None,
        permission: None,
        permission_source: Default::default(),
        allowed: false,
        force: false,
        remove_data: false,
        allow_hosts: Vec::new(),
        disallow_hosts: Vec::new(),
    };
    assert!(plugin_action_args(&action)
        .unwrap_err()
        .contains("pluginId"));
}

#[test]
fn plugin_action_activity_maps_install_remove_and_update() {
    let install = PluginManagerAction {
        from_dir: None,
        id_override: None,
        kind: "install".into(),
        plugin_id: Some("transcriber".into()),
        channel: None,
        url: None,
        name: None,
        permission: None,
        permission_source: Default::default(),
        allowed: false,
        force: false,
        remove_data: false,
        allow_hosts: Vec::new(),
        disallow_hosts: Vec::new(),
    };
    let install_activity = plugin_action_activity(&install).unwrap();
    assert_eq!(install_activity.plugin_id, "transcriber");
    assert_eq!(install_activity.kind, "install");
    assert_eq!(install_activity.label, "Installing transcriber…");

    let remove = PluginManagerAction {
        from_dir: None,
        id_override: None,
        kind: "remove".into(),
        plugin_id: Some("transcriber".into()),
        channel: None,
        url: None,
        name: None,
        permission: None,
        permission_source: Default::default(),
        allowed: false,
        force: false,
        remove_data: false,
        allow_hosts: Vec::new(),
        disallow_hosts: Vec::new(),
    };
    let remove_activity = plugin_action_activity(&remove).unwrap();
    assert_eq!(remove_activity.plugin_id, "transcriber");
    assert_eq!(remove_activity.kind, "uninstall");
    assert_eq!(remove_activity.label, "Uninstalling transcriber…");

    let update_system = PluginManagerAction {
        from_dir: None,
        id_override: None,
        kind: "update-system".into(),
        plugin_id: Some("codeterm-core".into()),
        channel: None,
        url: None,
        name: None,
        permission: None,
        permission_source: Default::default(),
        allowed: false,
        force: false,
        remove_data: false,
        allow_hosts: Vec::new(),
        disallow_hosts: Vec::new(),
    };
    let update_activity = plugin_action_activity(&update_system).unwrap();
    assert_eq!(update_activity.plugin_id, "codeterm-core");
    assert_eq!(update_activity.kind, "update");
    assert_eq!(update_activity.label, "Updating codeterm-core…");
}

#[test]
fn plugin_action_activity_maps_channel_operations() {
    let add = PluginManagerAction {
        from_dir: None,
        id_override: None,
        kind: "add-channel".into(),
        plugin_id: None,
        channel: None,
        url: Some("https://example.test/channel.json".into()),
        name: Some("demo".into()),
        permission: None,
        permission_source: Default::default(),
        allowed: false,
        force: false,
        remove_data: false,
        allow_hosts: Vec::new(),
        disallow_hosts: Vec::new(),
    };
    let add_activity = plugin_action_activity(&add).unwrap();
    assert_eq!(add_activity.plugin_id, "channel:demo");
    assert_eq!(add_activity.kind, "channel");
    assert_eq!(add_activity.label, "Adding channel demo…");

    let refresh = PluginManagerAction {
        from_dir: None,
        id_override: None,
        kind: "refresh-channel".into(),
        plugin_id: None,
        channel: Some("demo".into()),
        url: None,
        name: None,
        permission: None,
        permission_source: Default::default(),
        allowed: false,
        force: false,
        remove_data: false,
        allow_hosts: Vec::new(),
        disallow_hosts: Vec::new(),
    };
    let refresh_activity = plugin_action_activity(&refresh).unwrap();
    assert_eq!(refresh_activity.plugin_id, "channel:demo");
    assert_eq!(refresh_activity.kind, "channel");
    assert_eq!(refresh_activity.label, "Refreshing demo…");
}
