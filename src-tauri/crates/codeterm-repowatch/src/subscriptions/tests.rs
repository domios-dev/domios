use super::*;
use notify::{Config, EventHandler, WatcherKind};

#[derive(Default)]
struct Backend {
    active: BTreeMap<PathBuf, bool>,
    started: Vec<PathBuf>,
    stopped: Vec<PathBuf>,
}

#[test]
fn nested_ignored_directories_split_recursive_watches_and_removed_trees_coalesce() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
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
        self.started.push(path.to_path_buf());
        self.active
            .insert(path.to_path_buf(), mode == RecursiveMode::Recursive);
        Ok(())
    }
    fn unwatch(&mut self, path: &std::path::Path) -> notify::Result<()> {
        self.stopped.push(path.to_path_buf());
        self.active.remove(path);
        Ok(())
    }
    fn kind() -> WatcherKind {
        WatcherKind::NullWatcher
    }
}

#[test]
fn repository_subscriptions_exclude_ignored_trees_and_reconcile_topology() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::write(
        root.join(".gitignore"),
        "/generated/\n/vendor-*/\n!vendor-source/\n",
    )
    .unwrap();
    for name in ["generated", "vendor-cache", "vendor-source", "src", ".git"] {
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
    std::fs::create_dir(root.join(".git")).unwrap();
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
