use super::*;
use std::time::Duration;

#[derive(Default)]
struct NativeSubscriptions {
    active: HashMap<PathBuf, (RecursiveMode, u64)>,
    next: u64,
}

struct InjectedWatcher(Arc<Mutex<NativeSubscriptions>>);

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
    let watched = desired_watches(&[root.to_path_buf()]).unwrap();
    assert_eq!(watched.get(root), Some(&RecursiveMode::NonRecursive));
    assert_eq!(
        watched.get(&root.join("source")),
        Some(&RecursiveMode::Recursive)
    );
    assert!(!watched.contains_key(&root.join("output")));
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
    let events = FileEvents::new(vec![directory.path().to_path_buf()]).unwrap();
    let started = std::time::Instant::now();
    std::fs::write(directory.path().join("plugin.json"), "{}").unwrap();
    events
        .receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    eprintln!("plugin filesystem wake: {:?}", started.elapsed());
}

#[test]
fn a_missing_plugin_root_is_observed_through_its_existing_parent() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("plugins");
    let mut events = FileEvents::new(vec![root.clone()]).unwrap();
    std::fs::create_dir(&root).unwrap();
    events
        .receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    events.refresh().unwrap();
    assert_eq!(events.watched.get(&root), Some(&RecursiveMode::Recursive));
    std::fs::remove_dir(&root).unwrap();
    events
        .receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    events.refresh().unwrap();
    assert!(!events.watched.contains_key(&root));
}
