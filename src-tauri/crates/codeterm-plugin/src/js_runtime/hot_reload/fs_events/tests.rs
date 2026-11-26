use super::*;
use std::time::Duration;

#[derive(Default)]
struct NativeSubscriptions {
    active: HashMap<PathBuf, (RecursiveMode, u64)>,
    next: u64,
}

struct InjectedWatcher(Arc<Mutex<NativeSubscriptions>>);

struct DeferredWatcher {
    inner: InjectedWatcher,
    removals: Vec<PathBuf>,
}

impl PathWatcher for DeferredWatcher {
    fn watch(&mut self, path: &Path, mode: RecursiveMode) -> notify::Result<()> {
        for removed in self.removals.drain(..) {
            self.inner.unwatch(&removed)?;
        }
        self.inner.watch(path, mode)
    }

    fn unwatch(&mut self, path: &Path) -> notify::Result<()> {
        self.removals.push(path.to_path_buf());
        Ok(())
    }
}

pub(in crate::js_runtime::hot_reload) fn install_fixture(root: &Path) -> (FileEvents, HoldsPath) {
    let native = Arc::new(Mutex::new(NativeSubscriptions::default()));
    let (sender, receiver) = mpsc::sync_channel(1);
    let mut events = FileEvents {
        watcher: Box::new(DeferredWatcher {
            inner: InjectedWatcher(native.clone()),
            removals: Vec::new(),
        }),
        receiver,
        watched: HashMap::new(),
        state: EventState {
            roots: Arc::new(vec![root.canonicalize().unwrap()]),
            all_changed: Arc::new(AtomicBool::new(false)),
            invalidated: Arc::new(Mutex::new(Vec::new())),
            sender,
        },
    };
    events.refresh().unwrap();
    let holds = Arc::new(move |path: &Path| {
        native
            .lock()
            .active
            .keys()
            .any(|root| PathCase::native().within(root, path))
    });
    (events, holds)
}

struct VanishingWatcher {
    inner: InjectedWatcher,
    vanished: PathBuf,
    fail_existing: bool,
}

impl PathWatcher for VanishingWatcher {
    fn watch(&mut self, path: &Path, mode: RecursiveMode) -> notify::Result<()> {
        if path == self.vanished {
            if !self.fail_existing {
                std::fs::remove_dir(path).unwrap();
            }
            return Err(notify::Error::generic(
                "Input watch path is neither a file nor a directory.",
            ));
        }
        self.inner.watch(path, mode)
    }

    fn unwatch(&mut self, path: &Path) -> notify::Result<()> {
        self.inner.unwatch(path)
    }
}

#[test]
fn vanished_staging_subscription_is_skipped_but_existing_path_errors_propagate() {
    for fail_existing in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::write(root.join(".gitignore"), "output/\n").unwrap();
        std::fs::create_dir(root.join("output")).unwrap();
        let vanished = root.join("staging");
        std::fs::create_dir(&vanished).unwrap();
        let native = Arc::new(Mutex::new(NativeSubscriptions::default()));
        let (sender, receiver) = mpsc::sync_channel(1);
        let mut events = FileEvents {
            watcher: Box::new(VanishingWatcher {
                inner: InjectedWatcher(native.clone()),
                vanished: vanished.clone(),
                fail_existing,
            }),
            receiver,
            watched: HashMap::new(),
            state: EventState {
                roots: Arc::new(vec![root.into()]),
                all_changed: Arc::new(AtomicBool::new(false)),
                invalidated: Arc::new(Mutex::new(Vec::new())),
                sender,
            },
        };
        assert_eq!(events.refresh().is_err(), fail_existing);
        assert!(!events.watched.contains_key(&vanished));
        if !fail_existing {
            events.refresh().unwrap();
            assert_eq!(events.watched.get(root), Some(&RecursiveMode::NonRecursive));
            assert_eq!(events.watched.len(), native.lock().active.len());
        }
    }
}

impl PathWatcher for InjectedWatcher {
    fn watch(&mut self, path: &Path, mode: RecursiveMode) -> notify::Result<()> {
        let mut state = self.0.lock();
        state.next += 1;
        let generation = state.next;
        state.active.insert(path.to_path_buf(), (mode, generation));
        Ok(())
    }
    fn unwatch(&mut self, path: &Path) -> notify::Result<()> {
        self.0.lock().active.remove(path);
        Ok(())
    }
}

fn fire(
    native: &Arc<Mutex<NativeSubscriptions>>,
    state: &EventState,
    path: &Path,
    kind: EventKind,
) {
    if native.lock().active.iter().any(|(root, (mode, _))| {
        PathCase::native().same(path, root)
            || (*mode == RecursiveMode::Recursive && PathCase::native().within(path, root))
            || (*mode == RecursiveMode::NonRecursive && PathCase::native().direct_child(path, root))
    }) {
        state.publish(Ok(Event::new(kind).add_path(path.to_path_buf())));
    }
}

#[test]
fn replaced_descendant_rearms_and_subsequent_edits_wake_without_restarting_siblings() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::write(root.join(".gitignore"), "output/\n").unwrap();
    for name in ["source", "sibling", "output"] {
        std::fs::create_dir(root.join(name)).unwrap();
    }
    let native = Arc::new(Mutex::new(NativeSubscriptions::default()));
    let (sender, receiver) = mpsc::sync_channel(1);
    let state = EventState {
        roots: Arc::new(vec![root.into()]),
        all_changed: Arc::new(AtomicBool::new(false)),
        invalidated: Arc::new(Mutex::new(Vec::new())),
        sender,
    };
    let mut events = FileEvents {
        watcher: Box::new(InjectedWatcher(native.clone())),
        receiver,
        watched: HashMap::new(),
        state: state.clone(),
    };
    events.refresh().unwrap();
    let sibling = native.lock().active[&root.join("sibling")].1;
    let source = native.lock().active[&root.join("source")].1;
    std::fs::rename(root.join("source"), root.join("old-source")).unwrap();
    std::fs::create_dir(root.join("source")).unwrap();
    native.lock().active.remove(&root.join("source"));
    fire(
        &native,
        &state,
        &root.join("source"),
        EventKind::Modify(notify::event::ModifyKind::Name(
            notify::event::RenameMode::Both,
        )),
    );
    events.refresh().unwrap();
    assert!(events.receiver.try_recv().is_ok());
    assert_ne!(native.lock().active[&root.join("source")].1, source);
    assert_eq!(native.lock().active[&root.join("sibling")].1, sibling);
    std::fs::write(root.join("source/plugin.js"), "replacement").unwrap();
    fire(
        &native,
        &state,
        &root.join("source/plugin.js"),
        EventKind::Modify(notify::event::ModifyKind::Data(
            notify::event::DataChange::Any,
        )),
    );
    assert!(events.receiver.try_recv().is_ok());
}

#[test]
fn windows_mixed_case_descendant_events_remain_relevant() {
    let roots = vec![PathBuf::from("C:/Repo/Plugins")];
    let event = Event::new(EventKind::Modify(notify::event::ModifyKind::Name(
        notify::event::RenameMode::Both,
    )))
    .add_path("c:/REPO/plugins/Source".into())
    .add_path("C:/Repo/PLUGINS/source".into());
    assert!(relevant_with_case(&event, &roots, PathCase::Insensitive));
    assert!(!relevant_with_case(
        &event,
        &["C:/Repo/Other".into()],
        PathCase::Insensitive
    ));
}

#[test]
fn plugin_watch_roots_partition_ignored_top_level_directories() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::write(root.join(".gitignore"), "/output/\n").unwrap();
    std::fs::create_dir(root.join("output")).unwrap();
    std::fs::create_dir(root.join("source")).unwrap();
    std::fs::create_dir(root.join("source.tmp-install")).unwrap();
    std::fs::create_dir(root.join("source.rollback.tmp-install")).unwrap();
    let watched = desired_watches(&[root.to_path_buf()]).unwrap();
    assert_eq!(watched.get(root), Some(&RecursiveMode::NonRecursive));
    assert_eq!(
        watched.get(&root.join("source")),
        Some(&RecursiveMode::Recursive)
    );
    assert!(!watched.contains_key(&root.join("output")));
    assert!(!watched.contains_key(&root.join("source.tmp-install")));
    assert!(!watched.contains_key(&root.join("source.rollback.tmp-install")));
}

#[test]
fn plugin_reads_and_unrelated_files_do_not_schedule_reloads() {
    let roots = vec![PathBuf::from("/plugins")];
    let read = Event::new(EventKind::Access(AccessKind::Read))
        .add_path("/plugins/provider/plugin.js".into());
    let unrelated = Event::new(EventKind::Any).add_path("/sessions/output.log".into());
    let changed = Event::new(EventKind::Any).add_path("/plugins/provider/plugin.js".into());
    assert!(!relevant(&read, &roots));
    assert!(!relevant(&unrelated, &roots));
    assert!(relevant(&changed, &roots));
}

#[test]
fn filesystem_changes_wake_the_reconciler_without_a_polling_tick() {
    let directory = tempfile::tempdir().unwrap();
    let root = aliased_root(&directory);
    let events = FileEvents::new(vec![root.clone()]).unwrap();
    let started = std::time::Instant::now();
    std::fs::write(root.join("plugin.json"), "{}").unwrap();
    events
        .receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    eprintln!("plugin filesystem wake: {:?}", started.elapsed());
}

#[test]
fn a_missing_plugin_root_is_observed_through_its_existing_parent() {
    let directory = tempfile::tempdir().unwrap();
    let root = aliased_root(&directory).join("plugins");
    let mut events = FileEvents::new(vec![root.clone()]).unwrap();
    std::fs::create_dir(&root).unwrap();
    let canonical = root.canonicalize().unwrap();
    events
        .receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    events.refresh().unwrap();
    assert_eq!(
        events.watched.get(&canonical),
        Some(&RecursiveMode::NonRecursive)
    );
    std::fs::remove_dir(&root).unwrap();
    events
        .receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    events.refresh().unwrap();
    assert!(!events.watched.contains_key(&canonical));
}

fn aliased_root(directory: &tempfile::TempDir) -> PathBuf {
    let target = directory.path().join("target");
    std::fs::create_dir(&target).unwrap();
    #[cfg(unix)]
    {
        let alias = directory.path().join("alias");
        std::os::unix::fs::symlink(&target, &alias).unwrap();
        alias
    }
    #[cfg(not(unix))]
    {
        target
    }
}

#[test]
fn missing_descendants_keep_the_canonical_existing_ancestor() {
    let directory = tempfile::tempdir().unwrap();
    let root = aliased_root(&directory);
    assert_eq!(
        canonical_root(&root.join("missing/nested/plugins")).unwrap(),
        root.canonicalize().unwrap().join("missing/nested/plugins")
    );
}

#[test]
fn desired_watches_partitions_plain_root_and_never_watches_temp_or_rollback_dirs() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::create_dir(root.join("bitwarden")).unwrap();
    std::fs::create_dir(root.join("git")).unwrap();
    std::fs::create_dir(root.join("bitwarden.tmp-install")).unwrap();
    std::fs::create_dir(root.join("bitwarden.rollback.tmp-install")).unwrap();
    let watched = desired_watches(&[root.to_path_buf()]).unwrap();
    assert_eq!(watched.get(root), Some(&RecursiveMode::NonRecursive));
    assert_eq!(
        watched.get(&root.join("bitwarden")),
        Some(&RecursiveMode::Recursive)
    );
    assert_eq!(
        watched.get(&root.join("git")),
        Some(&RecursiveMode::Recursive)
    );
    assert!(!watched.contains_key(&root.join("bitwarden.tmp-install")));
    assert!(!watched.contains_key(&root.join("bitwarden.rollback.tmp-install")));
}

struct InotifyFaultWatcher {
    inner: InjectedWatcher,
    next_unwatch_error: Arc<Mutex<Option<notify::Error>>>,
}

impl PathWatcher for InotifyFaultWatcher {
    fn watch(&mut self, path: &Path, mode: RecursiveMode) -> notify::Result<()> {
        self.inner.watch(path, mode)
    }

    fn unwatch(&mut self, path: &Path) -> notify::Result<()> {
        self.inner.unwatch(path)?;
        if let Some(error) = self.next_unwatch_error.lock().take() {
            return Err(error);
        }
        Ok(())
    }
}

#[test]
fn release_bundle_is_idempotent_on_missing_or_invalidated_watches_and_rearms_root() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let bitwarden = root.join("bitwarden");
    let git = root.join("git");
    std::fs::create_dir(&bitwarden).unwrap();
    std::fs::create_dir(&git).unwrap();
    let native = Arc::new(Mutex::new(NativeSubscriptions::default()));
    let next_error = Arc::new(Mutex::new(None));
    let (sender, receiver) = mpsc::sync_channel(1);
    let mut events = FileEvents {
        watcher: Box::new(InotifyFaultWatcher {
            inner: InjectedWatcher(native.clone()),
            next_unwatch_error: next_error.clone(),
        }),
        receiver,
        watched: HashMap::new(),
        state: EventState {
            roots: Arc::new(vec![root.clone()]),
            all_changed: Arc::new(AtomicBool::new(false)),
            invalidated: Arc::new(Mutex::new(Vec::new())),
            sender,
        },
    };
    events.refresh().unwrap();

    let errors = [
        notify::Error::io(std::io::Error::from(std::io::ErrorKind::InvalidInput))
            .add_path(root.join("bitwarden.rollback.tmp-install")),
        notify::Error::watch_not_found().add_path(root.clone()),
        notify::Error::path_not_found().add_path(git.clone()),
        notify::Error::io(std::io::Error::from(std::io::ErrorKind::NotFound)).add_path(git.clone()),
    ];
    for (index, error) in errors.into_iter().enumerate() {
        let target = if index % 2 == 0 { &bitwarden } else { &git };
        *next_error.lock() = Some(error);
        events.release_bundle(target).unwrap();
        events.rearm_after_install(target).unwrap();
        assert_eq!(
            events.watched.get(&root),
            Some(&RecursiveMode::NonRecursive)
        );
        assert_eq!(events.watched.get(target), Some(&RecursiveMode::Recursive));
        assert!(native.lock().active.contains_key(&root));
        assert!(native.lock().active.contains_key(target));
    }
}

#[test]
fn two_consecutive_live_swaps_succeed_on_platform_watcher_without_watching_temp_dirs() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("plugins");
    let bitwarden = root.join("bitwarden");
    let git = root.join("git");
    std::fs::create_dir_all(&bitwarden).unwrap();
    std::fs::create_dir_all(&git).unwrap();
    std::fs::write(bitwarden.join("plugin.js"), "v1").unwrap();
    std::fs::write(git.join("plugin.js"), "v1").unwrap();

    let mut events = FileEvents::new(vec![root.clone()]).unwrap();
    let canonical_root = root.canonicalize().unwrap();
    assert_eq!(
        events.watched.get(&canonical_root),
        Some(&RecursiveMode::NonRecursive)
    );

    for (name, target) in [("bitwarden", &bitwarden), ("git", &git)] {
        let staging = root.join(format!("{name}.tmp-install"));
        let backup = root.join(format!("{name}.rollback.tmp-install"));
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("plugin.js"), "v2").unwrap();

        events.release_bundle(target).unwrap();
        std::fs::rename(target, &backup).unwrap();
        std::fs::rename(&staging, target).unwrap();
        std::fs::remove_dir_all(&backup).unwrap();
        events.rearm_after_install(target).unwrap();

        assert_eq!(
            events.watched.get(&canonical_root),
            Some(&RecursiveMode::NonRecursive)
        );
        assert!(events
            .watched
            .keys()
            .all(|path| !path.to_string_lossy().contains(".tmp-install")));
        assert_eq!(
            events.watched.get(&target.canonicalize().unwrap()),
            Some(&RecursiveMode::Recursive)
        );
    }

    while events.receiver.try_recv().is_ok() {}
    std::fs::write(git.join("plugin.js"), "v3").unwrap();
    events
        .receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
}
