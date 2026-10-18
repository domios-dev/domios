use super::*;
use notify::event::{AccessKind, CreateKind, DataChange, RemoveKind};
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test]
async fn native_config_and_auth_events_refresh_the_warm_authoritative_catalogue() {
    use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.json");
    let auth = dir.path().join("auth.json");
    std::fs::write(&config, b"old").unwrap();
    std::fs::write(&auth, b"a").unwrap();
    let id = "native-catalogue-watch-fixture";
    let manifest = serde_json::json!({
        "id": id, "version":"0.1.0", "displayName":"Fixture",
        "credentials":[{"id":"account", "file":auth}],
        "modelCatalogue":{"authority":"authoritative", "liveDiscoveryRequired":true, "configFiles":[config]},
    }).to_string();
    let vm = PluginVm::from_source_with(id, r#"
        module.exports.default = {
            discoverModels: function(stamp) {
                const files = JSON.parse(stamp).files;
                return [{id: "fixture/model-" + files[0][1] + "-" + files[1][1], displayName: "Fixture"}];
            }
        };
    "#.into(), Some(manifest.clone())).unwrap();
    let provider: Arc<dyn AgentProvider> = Arc::new(JsProvider::new(
        PluginManifest::from_json_str(&manifest).unwrap(),
        Arc::new(vm),
    ));
    model_catalog::refresh_models(provider.clone(), true)
        .await
        .unwrap();
    assert!(model_catalog::resolve_catalog_model(id, "fixture/model-1-3").is_some());
    install(provider.clone());
    let mut changes = model_catalog::subscribe_changes();
    for (path, data, expected) in [
        (&config, Some(&b"newest"[..]), "fixture/model-1-6"),
        (&auth, Some(&b"next"[..]), "fixture/model-4-6"),
        (&config, None, "fixture/model-4-null"),
    ] {
        match data {
            Some(data) => std::fs::write(path, data).unwrap(),
            None => std::fs::remove_file(path).unwrap(),
        }
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let change = changes.recv().await.unwrap();
                if change.provider == id
                    && model_catalog::resolve_catalog_model(id, expected).is_some()
                {
                    break;
                }
            }
        })
        .await
        .unwrap();
    }
    assert!(model_catalog::refresh_models(provider, false)
        .await
        .is_none());
    watchers().watchers.lock().remove(&format!("pty:{id}"));
    model_catalog::unregister(id);
}

#[test]
fn refresh_requests_coalesce_input_events_and_preserve_forced_work() {
    let mut queue = RefreshRequests::default();
    assert!(queue.begin("fixture", false));
    assert!(!queue.begin("fixture", true));
    assert!(!queue.begin("fixture", false));
    assert_eq!(queue.next("fixture"), Some(true));
    assert_eq!(queue.next("fixture"), None);
    assert!(queue.begin("fixture", false));
}

#[test]
fn a_declared_input_created_under_a_new_directory_is_observed() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("new").join("config.json");
    let registry = ProviderBinaryWatchers::default();
    let (tx, rx) = std::sync::mpsc::channel();
    registry.watch(
        "new-input".into(),
        vec![target.clone()],
        Arc::new(move || {
            let _ = tx.send(());
        }),
    );
    std::fs::create_dir(target.parent().unwrap()).unwrap();
    std::fs::write(&target, b"fixture").unwrap();
    rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
}

#[test]
fn repeated_install_keeps_one_watcher_and_updates_callback() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("input.json");
    let registry = ProviderBinaryWatchers::default();
    assert_eq!(
        registry.watch("provider".into(), vec![target.clone()], Arc::new(|| {})),
        WatchInstall::Installed(target.clone())
    );
    let first = registry.watchers.lock()["provider"].callback.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    for _ in 0..100 {
        let calls = calls.clone();
        assert_eq!(
            registry.watch(
                "provider".into(),
                vec![target.clone(), target.clone()],
                Arc::new(move || {
                    calls.fetch_add(1, Ordering::SeqCst);
                })
            ),
            WatchInstall::Reused
        );
    }
    let current = registry.watchers.lock();
    assert_eq!(current.len(), 1);
    assert!(Arc::ptr_eq(&first, &current["provider"].callback));
    let callback = first.lock().clone();
    drop(current);
    callback();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn changed_targets_replace_the_watch() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ProviderBinaryWatchers::default();
    registry.watch(
        "provider".into(),
        vec![dir.path().join("old")],
        Arc::new(|| {}),
    );
    let first = registry.watchers.lock()["provider"].callback.clone();
    let target = dir.path().join("new");
    assert_eq!(
        registry.watch("provider".into(), vec![target.clone()], Arc::new(|| {})),
        WatchInstall::Installed(target)
    );
    assert!(!Arc::ptr_eq(
        &first,
        &registry.watchers.lock()["provider"].callback
    ));
    assert_eq!(registry.watchers.lock().len(), 1);
    registry.watch("provider".into(), vec![], Arc::new(|| {}));
    assert!(registry.watchers.lock().is_empty());
}

#[test]
fn only_declared_input_mutations_trigger_refresh() {
    let target = PathBuf::from("catalogue/input");
    let targets = vec![target.clone()];
    for kind in [
        EventKind::Access(AccessKind::Read),
        EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime)),
    ] {
        assert!(!changes_input(
            &Event::new(kind).add_path(target.clone()),
            &targets
        ));
    }
    for kind in [
        EventKind::Create(CreateKind::File),
        EventKind::Modify(ModifyKind::Data(DataChange::Any)),
        EventKind::Remove(RemoveKind::File),
    ] {
        assert!(changes_input(
            &Event::new(kind).add_path(target.clone()),
            &targets
        ));
        assert!(!changes_input(
            &Event::new(kind).add_path(PathBuf::from("catalogue/unrelated")),
            &targets
        ));
    }
}

#[test]
fn stamps_ignore_reads_but_detect_creation_write_and_removal() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("input");
    let targets = vec![target.clone()];
    let missing = input_stamps(&targets);
    std::fs::write(&target, b"first").unwrap();
    let created = input_stamps(&targets);
    assert!(created != missing);
    let _ = std::fs::read(&target).unwrap();
    assert!(input_stamps(&targets) == created);
    std::fs::write(&target, b"different").unwrap();
    assert!(input_stamps(&targets) != created);
    std::fs::remove_file(&target).unwrap();
    assert!(input_stamps(&targets) == missing);
}
