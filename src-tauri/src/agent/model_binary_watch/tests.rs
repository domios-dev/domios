use super::*;
use notify::event::{AccessKind, CreateKind, DataChange, RemoveKind};
use std::sync::atomic::{AtomicUsize, Ordering};

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
