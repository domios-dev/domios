use super::*;
use notify::{Config, EventHandler, WatcherKind};

#[derive(Default)]
struct Backend {
    active: BTreeMap<PathBuf, bool>,
    started: Vec<PathBuf>,
    stopped: Vec<PathBuf>,
    operations: Vec<(bool, PathBuf)>,
    on_watch: Option<Box<dyn FnMut(&std::path::Path) + Send>>,
}

#[test]
fn recursive_admission_disables_symlink_traversal() {
    assert!(!crate::backend::admission_config(Config::default()).follow_symlinks());
}

#[test]
fn nested_ignored_directories_split_recursive_watches_and_removed_trees_coalesce() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    git2::Repository::init(root).unwrap();
    std::fs::write(root.join(".gitignore"), "target/\nnode_modules/\n").unwrap();
    for path in [
        "src-tauri/src",
        "packages/x/node_modules/dependency",
        "packages/x/src",
    ] {
        std::fs::create_dir_all(root.join(path)).unwrap();
    }
    let specs = vec![WatchSpec::worktree(root.to_path_buf())];
    let mut backend = Backend::default();
    let mut active = BTreeMap::new();
    reconcile(&mut backend, &mut active, &specs).unwrap();
    assert_eq!(active.get(&root.join("src-tauri")), Some(&true));
    assert_eq!(active.get(&root.join("packages/x")), Some(&false));
    assert!(!active
        .keys()
        .any(|path| path.starts_with(root.join("packages/x/node_modules"))));
    std::fs::create_dir_all(root.join("src-tauri/target/debug")).unwrap();
    assert!(topology_changed(
        &specs,
        &active,
        &[root.join("src-tauri/target")]
    ));
    reconcile(&mut backend, &mut active, &specs).unwrap();
    assert_eq!(active.get(&root.join("src-tauri")), Some(&false));
    assert_eq!(active.get(&root.join("src-tauri/src")), Some(&true));
    assert!(!active
        .keys()
        .any(|path| path.starts_with(root.join("src-tauri/target"))));
    assert_eq!(backend.active, active);
    assert!(backend.stopped.contains(&root.join("src-tauri")));
    std::fs::remove_dir_all(root.join("src-tauri/target")).unwrap();
    assert!(topology_changed(
        &specs,
        &active,
        &[root.join("src-tauri/target")]
    ));
    reconcile(&mut backend, &mut active, &specs).unwrap();
    assert_eq!(active.get(&root.join("src-tauri")), Some(&true));
    assert!(!active.contains_key(&root.join("src-tauri/src")));
    assert_eq!(backend.active, active);
}

impl Watcher for Backend {
    fn new<F: EventHandler>(_: F, _: Config) -> notify::Result<Self> {
        Ok(Self::default())
    }
    fn watch(&mut self, path: &std::path::Path, mode: RecursiveMode) -> notify::Result<()> {
        if let Some(mut callback) = self.on_watch.take() {
            callback(path);
        }
        self.operations.push((true, path.to_path_buf()));
        self.started.push(path.to_path_buf());
        self.active
            .insert(path.to_path_buf(), mode == RecursiveMode::Recursive);
        Ok(())
    }
    fn unwatch(&mut self, path: &std::path::Path) -> notify::Result<()> {
        self.operations.push((false, path.to_path_buf()));
        self.stopped.push(path.to_path_buf());
        let recursive = self.active.remove(path) == Some(true);
        if recursive {
            self.active.retain(|child, _| !child.starts_with(path));
        }
        Ok(())
    }
    fn kind() -> WatcherKind {
        WatcherKind::NullWatcher
    }
}

#[cfg(unix)]
#[test]
fn scan_to_admission_symlink_replacement_never_reaches_the_native_backend() {
    let directory = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let root = directory.path();
    git2::Repository::init(root).unwrap();
    std::fs::create_dir(root.join("source")).unwrap();
    let source = root.join("source");
    let external_path = external.path().to_path_buf();
    let callback_source = source.clone();
    let mut backend = Backend {
        on_watch: Some(Box::new(move |_| {
            std::fs::remove_dir(&callback_source).unwrap();
            std::os::unix::fs::symlink(&external_path, &callback_source).unwrap();
        })),
        ..Backend::default()
    };
    let error = reconcile(
        &mut backend,
        &mut BTreeMap::new(),
        &[WatchSpec::worktree(root.into())],
    )
    .unwrap_err();
    assert!(error.contains("symlink"));
    assert!(!backend.started.contains(&source));
    assert!(!backend
        .started
        .iter()
        .any(|path| path.starts_with(external.path())));
}

#[cfg(unix)]
#[test]
fn external_and_loop_symlinks_are_pruned_before_recursive_admission() {
    let directory = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let root = directory.path();
    git2::Repository::init(root).unwrap();
    std::fs::create_dir_all(root.join("source/safe")).unwrap();
    std::os::unix::fs::symlink(external.path(), root.join("source/external")).unwrap();
    std::os::unix::fs::symlink(root, root.join("source/loop")).unwrap();
    let mut backend = Backend::default();
    let mut active = BTreeMap::new();
    reconcile(
        &mut backend,
        &mut active,
        &[WatchSpec::worktree(root.into())],
    )
    .unwrap();
    assert_eq!(active.get(&root.join("source")), Some(&false));
    assert_eq!(active.get(&root.join("source/safe")), Some(&true));
    assert!(!active
        .keys()
        .any(|path| path.starts_with(root.join("source/external"))
            || path.starts_with(root.join("source/loop"))));
}

#[test]
fn repository_subscriptions_exclude_ignored_trees_and_reconcile_topology() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    git2::Repository::init(root).unwrap();
    std::fs::write(
        root.join(".gitignore"),
        "/generated/\n/vendor-*/\n!vendor-source/\n",
    )
    .unwrap();
    for name in ["generated", "vendor-cache", "vendor-source", "src"] {
        std::fs::create_dir(root.join(name)).unwrap();
    }
    let specs = vec![WatchSpec::worktree(root.to_path_buf())];
    let mut backend = Backend::default();
    let mut active = BTreeMap::new();
    reconcile(&mut backend, &mut active, &specs).unwrap();
    assert_eq!(
        active,
        BTreeMap::from([
            (root.to_path_buf(), false),
            (root.join("src"), true),
            (root.join("vendor-source"), true)
        ])
    );
    assert_eq!(backend.active, active);
    assert_eq!(backend.started.len(), 3);
    reconcile(&mut backend, &mut active, &specs).unwrap();
    assert_eq!(backend.started.len(), 3);
    std::fs::create_dir(root.join("new-source")).unwrap();
    assert!(topology_changed(
        &specs,
        &active,
        &[root.join("new-source")]
    ));
    reconcile(&mut backend, &mut active, &specs).unwrap();
    assert_eq!(active.get(&root.join("new-source")), Some(&true));
    std::fs::remove_dir(root.join("src")).unwrap();
    reconcile(&mut backend, &mut active, &specs).unwrap();
    assert!(!active.contains_key(&root.join("src")));
    std::fs::write(
        root.join(".gitignore"),
        "/generated/\n/vendor-*/\n/new-source/\n",
    )
    .unwrap();
    assert!(topology_changed(
        &specs,
        &active,
        &[root.join(".gitignore")]
    ));
    reconcile(&mut backend, &mut active, &specs).unwrap();
    assert!(!active.contains_key(&root.join("new-source")));
    assert!(!backend.started.contains(&root.join("generated")));
    assert!(!backend.started.contains(&root.join("vendor-cache")));
    assert!(backend.stopped.contains(&root.join("new-source")));
}

#[test]
fn replaced_directory_subscriptions_are_rearmed_even_when_paths_are_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    git2::Repository::init(root).unwrap();
    std::fs::create_dir(root.join("source")).unwrap();
    let specs = vec![WatchSpec::worktree(root.to_path_buf())];
    let mut backend = Backend::default();
    let mut active = BTreeMap::new();
    reconcile(&mut backend, &mut active, &specs).unwrap();
    backend.active.remove(&root.join("source"));
    invalidate(&mut backend, &mut active, &[root.join("source")]).unwrap();
    reconcile(&mut backend, &mut active, &specs).unwrap();
    assert_eq!(backend.active, active);
    assert_eq!(
        backend
            .started
            .iter()
            .filter(|path| **path == root.join("source"))
            .count(),
        2
    );
}

#[test]
fn windows_mixed_case_events_and_case_only_renames_invalidate_the_same_subscription() {
    let root = PathBuf::from("C:/Repo");
    let source = root.join("Source");
    let sibling = root.join("Sibling");
    let specs = vec![WatchSpec::worktree(root.clone())];
    let mut active = BTreeMap::from([
        (root, false),
        (source.clone(), true),
        (sibling.clone(), true),
    ]);
    let mut backend = Backend {
        active: active.clone(),
        ..Backend::default()
    };
    let paths = vec![
        PathBuf::from("c:/REPO/Source"),
        PathBuf::from("C:/repo/source"),
    ];
    assert!(topology_changed_with_case(
        &specs,
        &active,
        &paths,
        PathCase::Insensitive
    ));
    invalidate_with_case(&mut backend, &mut active, &paths, PathCase::Insensitive).unwrap();
    assert!(!active.contains_key(&source));
    assert!(active.contains_key(&sibling));
    assert_eq!(backend.active, active);
}

#[test]
fn ignored_directories_preserve_tracked_inputs_and_git_filter_relevance() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let repo = git2::Repository::init(root).unwrap();
    std::fs::create_dir_all(root.join("archive/owned")).unwrap();
    std::fs::create_dir_all(root.join("archive/churn/deep")).unwrap();
    std::fs::write(root.join("archive/owned/later-ignored.txt"), "before").unwrap();
    let mut index = repo.index().unwrap();
    index
        .add_path(std::path::Path::new("archive/owned/later-ignored.txt"))
        .unwrap();
    std::fs::write(root.join(".gitignore"), "archive/\n").unwrap();
    std::fs::write(root.join("archive/owned/force-added.txt"), "before").unwrap();
    index
        .add_path(std::path::Path::new("archive/owned/force-added.txt"))
        .unwrap();
    index.write().unwrap();
    let active = expand(&[WatchSpec::worktree(root.to_path_buf())]).unwrap();
    assert_eq!(active.get(&root.join("archive")), Some(&false));
    assert_eq!(active.get(&root.join("archive/owned")), Some(&false));
    assert!(!active
        .keys()
        .any(|path| path.starts_with(root.join("archive/churn"))));
    let identity = crate::RepoIdentity::resolve(root).unwrap();
    for path in [
        "archive/owned/later-ignored.txt",
        "archive/owned/force-added.txt",
        "archive/owned",
    ] {
        assert!(crate::filter::batch_is_relevant(
            &identity,
            crate::filter::WatchHalf::Worktree,
            &[root.join(path)]
        )
        .unwrap());
    }
    assert!(!crate::filter::batch_is_relevant(
        &identity,
        crate::filter::WatchHalf::Worktree,
        &[root.join("archive/churn/deep/output")]
    )
    .unwrap());
}

struct ControlledSet {
    watcher: Backend,
    active: BTreeMap<PathBuf, bool>,
    specs: Vec<WatchSpec>,
    handler: crate::BatchHandler,
}

#[derive(Default)]
struct ControlledBackend(std::sync::Arc<parking_lot::Mutex<Option<ControlledSet>>>);

struct ControlledWatch(std::sync::Arc<parking_lot::Mutex<Option<ControlledSet>>>);

impl crate::ActiveWatch for ControlledWatch {
    fn update(&mut self, specs: &[WatchSpec]) -> Result<(), String> {
        let mut guard = self.0.lock();
        let set = guard.as_mut().unwrap();
        reconcile(&mut set.watcher, &mut set.active, specs)?;
        set.specs = specs.to_vec();
        Ok(())
    }
}

impl crate::WatchBackend for ControlledBackend {
    fn build(
        &self,
        specs: &[WatchSpec],
        handler: crate::BatchHandler,
    ) -> Result<Box<dyn crate::ActiveWatch>, String> {
        let mut watcher = Backend::default();
        let mut active = BTreeMap::new();
        reconcile(&mut watcher, &mut active, specs)?;
        *self.0.lock() = Some(ControlledSet {
            watcher,
            active,
            specs: specs.to_vec(),
            handler,
        });
        Ok(Box::new(ControlledWatch(self.0.clone())))
    }
}

#[test]
fn coverage_transition_invalidates_the_repository_after_a_concurrent_edit() {
    use crate::tests::support::{collected, registry_with, SinkEvent};
    use std::sync::Arc;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let repo = git2::Repository::init(root).unwrap();
    std::fs::create_dir_all(root.join("src-tauri/source")).unwrap();
    let file = root.join("src-tauri/source/tracked.txt");
    std::fs::write(&file, "before").unwrap();
    std::fs::write(root.join(".gitignore"), "target/\n").unwrap();
    let mut index = repo.index().unwrap();
    index
        .add_path(std::path::Path::new("src-tauri/source/tracked.txt"))
        .unwrap();
    index.write().unwrap();
    let backend = Arc::new(ControlledBackend::default());
    let (registry, events) = registry_with(backend.clone());
    let subscription = registry.subscribe(root).unwrap();
    std::fs::create_dir(root.join("src-tauri/target")).unwrap();
    let mut guard = backend.0.lock();
    let set = guard.as_mut().unwrap();
    let callback_file = file.clone();
    set.watcher.on_watch = Some(Box::new(move |_| {
        std::fs::write(&callback_file, "during-transition").unwrap()
    }));
    let paths = vec![root.join("src-tauri/target")];
    let signals = crate::backend::refresh_coverage(
        &mut set.watcher,
        &mut set.active,
        &set.specs,
        &paths,
        false,
    );
    for signal in signals {
        (set.handler)(signal);
    }
    (set.handler)(crate::WatchSignal::Batch(paths));
    let start = set
        .watcher
        .operations
        .iter()
        .position(|(add, path)| *add && path == &root.join("src-tauri/source"))
        .unwrap();
    let stop = set
        .watcher
        .operations
        .iter()
        .position(|(add, path)| !add && path == &root.join("src-tauri"))
        .unwrap();
    assert!(
        start < stop,
        "replacement coverage is admitted before old coverage is removed"
    );
    assert_eq!(set.watcher.active, set.active);
    assert_eq!(std::fs::read_to_string(file).unwrap(), "during-transition");
    assert_eq!(
        collected(&events),
        vec![SinkEvent::Changed(subscription.identity)]
    );
}
