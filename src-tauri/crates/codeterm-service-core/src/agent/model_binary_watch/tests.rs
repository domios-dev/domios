use super::*;
use notify::event::{AccessKind, CreateKind, DataChange, RemoveKind};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicUsize, Ordering};

// Bounds a missing event only; macOS FSEvents coalesces for about a second and lags further under load.
const WATCH_DEADLINE: std::time::Duration = std::time::Duration::from_secs(15);

#[test]
fn scoped_data_events_trigger_once_with_preserved_size_and_mtime() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["config", "auth"] {
        let path = dir.path().join(name);
        std::fs::write(&path, b"before").unwrap();
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        let targets = vec![path.clone()];
        let mut stamps = input_stamps(&targets);
        std::fs::write(&path, b"latest").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        assert!(!same_input_stamps(&input_stamps(&targets), &stamps));
        let event = Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content)))
            .add_path(path.clone());
        assert!(accept_input_event(&event, &targets, &mut stamps));
        assert!(!accept_input_event(&event, &targets, &mut stamps));
        let unchanged = Event::new(EventKind::Modify(ModifyKind::Metadata(
            MetadataKind::WriteTime,
        )))
        .add_path(path);
        assert!(!accept_input_event(&unchanged, &targets, &mut stamps));
        let unrelated = Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content)))
            .add_path(dir.path().join("unrelated"));
        assert!(!accept_input_event(&unrelated, &targets, &mut stamps));
    }
}

#[test]
fn input_change_gate_coalesces_duplicate_content_and_accepts_same_metadata_edits() {
    let modified = Some(std::time::UNIX_EPOCH);
    let previous = InputStamp {
        len: 6,
        modified: modified.clone(),
        content_digest: Some(Sha256::digest(b"before").into()),
    };
    let changed = InputStamp {
        len: 6,
        modified,
        content_digest: Some(Sha256::digest(b"latest").into()),
    };
    let identical_rewrite = InputStamp {
        len: 6,
        modified: Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1)),
        content_digest: Some(Sha256::digest(b"latest").into()),
    };

    assert!(!same_input_stamps(
        &[Some(previous)],
        &[Some(changed.clone())]
    ));
    assert!(same_input_stamps(
        &[Some(changed)],
        &[Some(identical_rewrite)]
    ));
}

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
    let vm = PluginVm::from_source_with(
        id,
        r#"
        let discoveries = 0;
        module.exports.default = {
            discoverModels: function(binaryFingerprint) {
                if (binaryFingerprint !== null && !/^[a-f0-9]{64}$/.test(binaryFingerprint)) {
                    throw new Error("invalid binary fingerprint");
                }
                discoveries += 1;
                return [{id: "fixture/model-" + discoveries, displayName: "Fixture"}];
            }
        };
    "#
        .into(),
        Some(manifest.clone()),
    )
    .unwrap();
    let provider: Arc<dyn AgentProvider> = Arc::new(JsProvider::new(
        PluginManifest::from_json_str(&manifest).unwrap(),
        Arc::new(vm),
    ));
    model_catalog::refresh_models(provider.clone(), true)
        .await
        .unwrap();
    assert!(model_catalog::resolve_catalog_model(id, "fixture/model-1").is_some());
    assert!(model_catalog::refresh_models(provider.clone(), false)
        .await
        .is_none());
    install(provider.clone());
    let mut changes = model_catalog::subscribe_changes();
    for (path, data) in [
        (&config, Some(&b"newest"[..])),
        (&auth, Some(&b"next"[..])),
        (&config, None),
    ] {
        let previous = model_catalog::resolve_models(id)[0].id.clone();
        match data {
            Some(data) => std::fs::write(path, data).unwrap(),
            None => std::fs::remove_file(path).unwrap(),
        }
        tokio::time::timeout(WATCH_DEADLINE, async {
            loop {
                let change = changes.recv().await.unwrap();
                if change.provider == id {
                    let models = model_catalog::resolve_models(id);
                    if models.first().is_some_and(|model| model.id != previous) {
                        break;
                    }
                }
            }
        })
        .await
        .unwrap();
    }
    watchers().watchers.lock().remove(&format!("pty:{id}"));
    model_catalog::unregister(id);
}

#[tokio::test]
async fn a_provider_cli_update_republishes_the_catalogue_probed_against_it() {
    use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
    let dir = tempfile::tempdir().unwrap();
    let cli = dir.path().join("fixture-cli");
    std::fs::write(&cli, b"v1").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let id = "cli-update-catalogue-watch-fixture";
    let manifest = serde_json::json!({
        "id": id, "version":"0.1.0", "displayName":"Fixture", "cliBinary": cli,
        "modelCatalogue":{"authority":"authoritative", "liveDiscoveryRequired":true},
    })
    .to_string();
    let vm = PluginVm::from_source_with(
        id,
        r#"
        module.exports.default = {
            discoverModels: function(binaryFingerprint) {
                if (binaryFingerprint === null) return null;
                return [{id: "fixture/" + binaryFingerprint.slice(0, 16), displayName: "Fixture"}];
            }
        };
    "#
        .into(),
        Some(manifest.clone()),
    )
    .unwrap();
    let provider: Arc<dyn AgentProvider> = Arc::new(JsProvider::new(
        PluginManifest::from_json_str(&manifest).unwrap(),
        Arc::new(vm),
    ));
    model_catalog::refresh_models(provider.clone(), true)
        .await
        .expect("a probe that resolves its CLI publishes");
    assert_eq!(model_catalog::last_refresh_failure(id), None);
    let before = model_catalog::resolve_models(id)[0].id.clone();
    install(provider.clone());
    let mut changes = model_catalog::subscribe_changes();
    std::fs::write(&cli, b"v2-updated-cli").unwrap();
    tokio::time::timeout(WATCH_DEADLINE, async {
        loop {
            let change = changes.recv().await.unwrap();
            if change.provider == id
                && model_catalog::resolve_models(id)
                    .first()
                    .is_some_and(|model| model.id != before)
            {
                break;
            }
        }
    })
    .await
    .expect("a rewritten provider CLI refreshes its catalogue");
    assert!(!model_catalog::provider_binary_changed(id));
    watchers().watchers.lock().remove(&format!("pty:{id}"));
    model_catalog::unregister(id);
}

#[test]
fn refresh_requests_coalesce_input_events_and_preserve_forced_work() {
    let mut queue = RefreshRequests::default();
    assert!(queue.begin("fixture", false));
    for _ in 0..19 {
        assert!(!queue.begin("fixture", true));
    }
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
        registry.admit("new-input".into()),
        vec![target.clone()],
        Arc::new(move || {
            let _ = tx.send(());
        }),
    );
    std::fs::create_dir(target.parent().unwrap()).unwrap();
    std::fs::write(&target, b"fixture").unwrap();
    rx.recv_timeout(WATCH_DEADLINE).unwrap();
}

#[test]
fn repeated_install_keeps_one_watcher_and_updates_callback() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("input.json");
    let registry = ProviderBinaryWatchers::default();
    assert_eq!(
        registry.watch(
            registry.admit("provider".into()),
            vec![target.clone()],
            Arc::new(|| {})
        ),
        WatchInstall::Installed(target.clone())
    );
    let first = registry.watchers.lock()["provider"].callback.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    for _ in 0..100 {
        let calls = calls.clone();
        assert_eq!(
            registry.watch(
                registry.admit("provider".into()),
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
    let callback = first.lock().clone().unwrap();
    drop(current);
    callback();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn changed_targets_replace_the_watch() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ProviderBinaryWatchers::default();
    registry.watch(
        registry.admit("provider".into()),
        vec![dir.path().join("old")],
        Arc::new(|| {}),
    );
    let first = registry.watchers.lock()["provider"].callback.clone();
    let target = dir.path().join("new");
    assert_eq!(
        registry.watch(
            registry.admit("provider".into()),
            vec![target.clone()],
            Arc::new(|| {})
        ),
        WatchInstall::Installed(target)
    );
    assert!(!Arc::ptr_eq(
        &first,
        &registry.watchers.lock()["provider"].callback
    ));
    assert_eq!(registry.watchers.lock().len(), 1);
    registry.watch(registry.admit("provider".into()), vec![], Arc::new(|| {}));
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
    assert!(changes_input(
        &Event::new(EventKind::Create(CreateKind::Folder)).add_path(PathBuf::from("catalogue")),
        &targets
    ));
    assert!(!changes_input(
        &Event::new(EventKind::Create(CreateKind::Folder))
            .add_path(PathBuf::from("catalogue/sibling")),
        &targets
    ));
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

type Handlers = Arc<Mutex<Vec<StreamHandler>>>;

fn recording(
    handlers: &Handlers,
) -> impl Fn(&[(PathBuf, bool)], StreamHandler) -> Option<Box<dyn Send>> + '_ {
    move |_: &[(PathBuf, bool)], handler: StreamHandler| {
        handlers.lock().push(handler);
        Some(Box::new(()))
    }
}

fn counted() -> (Arc<AtomicUsize>, OnChange) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let on_change: OnChange = Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    (calls, on_change)
}

fn deliver_change(handlers: &Handlers, index: usize, target: &std::path::Path, contents: &[u8]) {
    std::fs::write(target, contents).unwrap();
    let event = Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Any)))
        .add_path(target.to_path_buf());
    let mut handlers = handlers.lock();
    let handler = &mut handlers[index];
    handler(Ok(event));
}

fn published_callback(registry: &ProviderBinaryWatchers) -> OnChange {
    registry.watchers.lock()["provider"]
        .callback
        .lock()
        .clone()
        .expect("published watch has an active callback")
}

#[test]
fn startup_perf_slow_old_watch_install_never_replaces_newer_same_target_watch() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("input.json");
    std::fs::write(&target, b"initial").unwrap();
    let registry = Arc::new(ProviderBinaryWatchers::default());
    let handlers: Handlers = Arc::default();
    let (old_calls, old_change) = counted();
    let (new_calls, new_change) = counted();
    let (building_tx, building_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();

    let old_admission = registry.admit("provider".into());
    let old = std::thread::spawn({
        let registry = registry.clone();
        let handlers = handlers.clone();
        let target = target.clone();
        move || {
            let slow = |_: &[(PathBuf, bool)], handler: StreamHandler| {
                handlers.lock().push(handler);
                deliver_change(&handlers, 0, &target, b"while building");
                building_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Some(Box::new(()) as Box<dyn Send>)
            };
            registry.watch_with(old_admission, vec![target.clone()], old_change, &slow)
        }
    });
    building_rx
        .recv_timeout(WATCH_DEADLINE)
        .expect("old install reached its stream start");

    let new_admission = registry.admit("provider".into());
    assert_eq!(
        registry.watch_with(
            new_admission,
            vec![target.clone()],
            new_change,
            &recording(&handlers)
        ),
        WatchInstall::Installed(target.clone())
    );
    release_tx.send(()).unwrap();
    assert_eq!(old.join().unwrap(), WatchInstall::Superseded);

    published_callback(&registry)();
    assert_eq!(new_calls.load(Ordering::SeqCst), 1);
    deliver_change(&handlers, 1, &target, b"after publish");
    assert_eq!(new_calls.load(Ordering::SeqCst), 2);
    deliver_change(&handlers, 0, &target, b"late event on the dropped stream");
    assert_eq!(
        old_calls.load(Ordering::SeqCst),
        0,
        "a stream that never published never calls out"
    );
}

#[test]
fn startup_perf_slow_old_unavailable_install_never_removes_newer_watch() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("input.json");
    let registry = Arc::new(ProviderBinaryWatchers::default());
    let handlers: Handlers = Arc::default();
    let (old_calls, old_change) = counted();
    let (new_calls, new_change) = counted();
    let (probed_tx, probed_rx) = std::sync::mpsc::channel::<()>();

    let old_admission = registry.admit("provider".into());
    let stale_reuse = registry.admit("provider".into());
    let old = std::thread::spawn({
        let registry = registry.clone();
        let handlers = handlers.clone();
        let old_change = old_change.clone();
        move || {
            // Still resolving its inputs when the newer request publishes.
            probed_rx.recv().unwrap();
            registry.watch_with(old_admission, vec![], old_change, &recording(&handlers))
        }
    });

    let new_admission = registry.admit("provider".into());
    assert_eq!(
        registry.watch_with(
            new_admission,
            vec![target.clone()],
            new_change,
            &recording(&handlers)
        ),
        WatchInstall::Installed(target.clone())
    );
    probed_tx.send(()).unwrap();
    assert_eq!(old.join().unwrap(), WatchInstall::Superseded);
    assert_eq!(
        registry.watch_with(
            stale_reuse,
            vec![target.clone()],
            old_change,
            &recording(&handlers)
        ),
        WatchInstall::Superseded
    );

    assert_eq!(registry.watchers.lock().len(), 1);
    published_callback(&registry)();
    assert_eq!(new_calls.load(Ordering::SeqCst), 1);
    assert_eq!(old_calls.load(Ordering::SeqCst), 0);
}
