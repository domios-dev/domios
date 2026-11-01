use super::*;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

#[derive(Default)]
struct InjectedBackend {
    directories: parking_lot::Mutex<std::collections::HashSet<PathBuf>>,
    started: parking_lot::Mutex<Vec<PathBuf>>,
    stopped: Arc<std::sync::atomic::AtomicUsize>,
}

struct InjectedWatch(Arc<std::sync::atomic::AtomicUsize>);

impl Drop for InjectedWatch {
    fn drop(&mut self) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

impl DirectoryBackend for InjectedBackend {
    fn nearest(&self, source: &Path) -> Result<PathBuf, String> {
        source
            .ancestors()
            .skip(1)
            .find(|path| self.directories.lock().contains(*path))
            .map(Path::to_path_buf)
            .ok_or_else(|| "no directory".into())
    }

    fn watch(
        &self,
        root: &Path,
        mode: RecursiveMode,
        _: u64,
        _: &Sender<Command>,
    ) -> Result<Box<dyn Send>, String> {
        assert_eq!(mode, RecursiveMode::NonRecursive);
        self.started.lock().push(root.to_path_buf());
        Ok(Box::new(InjectedWatch(self.stopped.clone())))
    }
}

#[test]
fn source_subscriptions_descend_nonrecursively_with_injected_backend() {
    let backend = Arc::new(InjectedBackend::default());
    let root = PathBuf::from("sessions");
    let project = root.join("project");
    let session = project.join("own");
    let source = session.join("turn.jsonl");
    backend.directories.lock().insert(root.clone());
    let mut registry = Registry {
        backend: backend.clone(),
        ..Registry::default()
    };
    let (commands, _) = mpsc::channel();
    let (wake, changes) = mpsc::sync_channel(1);
    let first = registry
        .subscribe(
            source.clone(),
            wake.clone(),
            Arc::new(AtomicBool::new(true)),
            &commands,
        )
        .unwrap();
    let second = registry
        .subscribe(
            source.clone(),
            wake,
            Arc::new(AtomicBool::new(true)),
            &commands,
        )
        .unwrap();
    assert_eq!(*backend.started.lock(), vec![root.clone()]);
    for (old, next) in [(&root, &project), (&project, &session)] {
        backend.directories.lock().insert(next.clone());
        let generation = registry.roots[old].generation;
        registry.event(
            old.clone(),
            generation,
            Ok(
                Event::new(EventKind::Create(notify::event::CreateKind::Folder))
                    .add_path(next.clone()),
            ),
            &commands,
        );
        assert!(changes.try_recv().is_ok());
        assert_eq!(registry.roots.len(), 1);
        assert!(registry.roots.contains_key(next));
        assert!(registry
            .subscribers
            .values()
            .all(|entry| entry.root == *next));
    }
    assert_eq!(*backend.started.lock(), vec![root, project, session]);
    registry.subscribers.remove(&first);
    registry.prune();
    assert_eq!(registry.roots.len(), 1);
    registry.subscribers.remove(&second);
    registry.prune();
    assert!(registry.roots.is_empty());
    assert_eq!(backend.stopped.load(std::sync::atomic::Ordering::SeqCst), 3);
}

fn receive(events: &Receiver<()>) {
    events.recv_timeout(Duration::from_secs(2)).unwrap();
}

fn roots(guard: &WatchGuard, directory: &Path) -> Vec<(PathBuf, usize)> {
    let (reply, result) = mpsc::channel();
    guard.commands.send(Command::Inspect(reply)).unwrap();
    result
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .into_iter()
        .filter(|(root, _)| root.starts_with(directory))
        .collect()
}

fn await_root(guard: &WatchGuard, events: &Receiver<()>, directory: &Path, expected: &Path) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if roots(guard, directory) == vec![(expected.to_path_buf(), 1)] {
            return;
        }
        events
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
    }
}

#[test]
fn append_and_materialization_wake_only_the_claimed_source() {
    let source = Path::new("sessions/own/turn.jsonl");
    assert!(affects_source(
        &Event::new(EventKind::Modify(notify::event::ModifyKind::Any)).add_path(source.into()),
        source
    ));
    assert!(affects_source(
        &Event::new(EventKind::Create(notify::event::CreateKind::Folder))
            .add_path("sessions/own".into()),
        source
    ));
    assert!(!affects_source(
        &Event::new(EventKind::Modify(notify::event::ModifyKind::Any))
            .add_path("sessions/foreign/turn.jsonl".into()),
        source
    ));
    assert!(!affects_source(
        &Event::new(EventKind::Access(notify::event::AccessKind::Any)).add_path(source.into()),
        source
    ));
}

#[test]
fn sibling_subscribers_share_one_native_watcher_and_drop_independently() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().canonicalize().unwrap();
    let first = directory.join("first.jsonl");
    let second = directory.join("second.jsonl");
    std::fs::write(&first, b"").unwrap();
    std::fs::write(&second, b"").unwrap();
    let (wake_first, events_first) = mpsc::sync_channel(1);
    let (wake_second, events_second) = mpsc::sync_channel(1);
    let first_guard = watch(&first, wake_first).unwrap();
    let second_guard = watch(&second, wake_second).unwrap();
    assert_eq!(
        roots(&first_guard, &directory),
        vec![(directory.clone(), 2)]
    );
    std::fs::write(&first, b"first\n").unwrap();
    receive(&events_first);
    assert!(events_second.try_recv().is_err());
    drop(first_guard);
    assert_eq!(
        roots(&second_guard, &directory),
        vec![(directory.clone(), 1)]
    );
    std::fs::write(&second, b"second\n").unwrap();
    receive(&events_second);
    let commands = second_guard.commands.clone();
    drop(second_guard);
    let (reply, result) = mpsc::channel();
    commands.send(Command::Inspect(reply)).unwrap();
    assert!(result
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .iter()
        .all(|(root, _)| !root.starts_with(&directory)));
}

#[test]
fn missing_source_descends_and_survives_file_recreation() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().canonicalize().unwrap();
    let session = directory.join("sessions").join("own");
    let source = session.join("turn.jsonl");
    let (wake, events) = mpsc::sync_channel(1);
    let guard = watch(&source, wake).unwrap();
    assert_eq!(roots(&guard, &directory), vec![(directory.clone(), 1)]);
    std::fs::create_dir(directory.join("sessions")).unwrap();
    await_root(&guard, &events, &directory, &directory.join("sessions"));
    std::fs::create_dir(&session).unwrap();
    await_root(&guard, &events, &directory, &session);
    while events.try_recv().is_ok() {}
    std::fs::write(&source, b"first\n").unwrap();
    receive(&events);
    std::fs::remove_file(&source).unwrap();
    receive(&events);
    std::fs::write(&source, b"replacement\n").unwrap();
    receive(&events);
    assert_eq!(roots(&guard, &directory), vec![(session, 1)]);
}

#[test]
fn materialized_branches_converge_on_the_same_shared_root() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().canonicalize().unwrap();
    let session = directory.join("sessions");
    let (wake_first, events_first) = mpsc::sync_channel(1);
    let (wake_second, events_second) = mpsc::sync_channel(1);
    let first = watch(&session.join("first.jsonl"), wake_first).unwrap();
    let second = watch(&session.join("second.jsonl"), wake_second).unwrap();
    std::fs::create_dir(&session).unwrap();
    receive(&events_first);
    receive(&events_second);
    assert_eq!(roots(&first, &directory), vec![(session, 2)]);
    drop(second);
}

#[test]
#[cfg(unix)]
fn filesystem_append_wakes_through_a_directory_alias() {
    let temp = tempfile::tempdir().unwrap();
    let real = temp.path().join("real");
    let alias = temp.path().join("alias");
    std::fs::create_dir(&real).unwrap();
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    let source = alias.join("own.jsonl");
    std::fs::write(&source, b"").unwrap();
    let (wake, events) = mpsc::sync_channel(1);
    let _watcher = watch(&source, wake).unwrap();
    std::fs::write(&source, b"opaque-turn\n").unwrap();
    receive(&events);
}

use notify::event::Flag;

#[test]
fn rescan_selects_every_own_source_on_the_affected_root() {
    let mut registry = Registry::default();
    let root = PathBuf::from("sessions/own");
    let foreign_root = PathBuf::from("sessions/foreign");
    let (first_wake, _) = mpsc::sync_channel(1);
    let (second_wake, _) = mpsc::sync_channel(1);
    let (foreign_wake, _) = mpsc::sync_channel(1);
    for (id, root, wake) in [
        (1, &root, first_wake),
        (2, &root, second_wake),
        (3, &foreign_root, foreign_wake),
    ] {
        registry.subscribers.insert(
            id,
            Subscriber {
                source: root.join(format!("{id}.jsonl")),
                root: root.clone(),
                wake,
                armed: Arc::new(AtomicBool::new(true)),
            },
        );
    }
    let rescan = Event::new(EventKind::Other).set_flag(Flag::Rescan);
    assert!(rescan.paths.is_empty());
    assert!(affects_source(&rescan, &root.join("1.jsonl")));
    assert!(!affects_source(
        &Event::new(EventKind::Other),
        &root.join("1.jsonl")
    ));
    let mut affected = registry.affected_subscribers(&root, &Ok(rescan));
    affected.sort_unstable();
    assert_eq!(affected, vec![1, 2]);
    let own_change = Event::new(EventKind::Modify(notify::event::ModifyKind::Any))
        .add_path(root.join("1.jsonl"));
    assert_eq!(
        registry.affected_subscribers(&root, &Ok(own_change)),
        vec![1]
    );
}
