use super::*;
use notify::event::{DataChange, ModifyKind, RemoveKind, RenameMode};
use notify::{Config, Event, EventHandler, EventKind, RecursiveMode, WatcherKind};

#[derive(Default)]
struct Backend {
    started: Vec<PathBuf>,
    stopped: Vec<PathBuf>,
}

impl Watcher for Backend {
    fn new<F: EventHandler>(_: F, _: Config) -> notify::Result<Self> {
        Ok(Self::default())
    }
    fn watch(&mut self, path: &Path, _: RecursiveMode) -> notify::Result<()> {
        self.started.push(path.to_path_buf());
        Ok(())
    }
    fn unwatch(&mut self, path: &Path) -> notify::Result<()> {
        self.stopped.push(path.to_path_buf());
        Ok(())
    }
    fn kind() -> WatcherKind {
        WatcherKind::NullWatcher
    }
}

struct Fixture {
    directory: tempfile::TempDir,
    repo: git2::Repository,
    specs: Vec<WatchSpec>,
    coverage: Coverage,
    active: BTreeMap<PathBuf, bool>,
    backend: Backend,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let repo = git2::Repository::init(root).unwrap();
        std::fs::create_dir(root.join("source")).unwrap();
        std::fs::create_dir_all(root.join("ignored/deep")).unwrap();
        std::fs::write(root.join(".gitignore"), "ignored/\n").unwrap();
        let specs = vec![
            WatchSpec::worktree(root.into()),
            WatchSpec::shallow(repo.path().into()),
        ];
        let mut fixture = Self {
            directory,
            repo,
            specs,
            coverage: Coverage::default(),
            active: BTreeMap::new(),
            backend: Backend::default(),
        };
        fixture
            .coverage
            .reconcile(
                &mut fixture.backend,
                &mut fixture.active,
                &fixture.specs,
                &[],
            )
            .unwrap();
        fixture
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.directory.path().join(relative)
    }

    fn apply(&mut self, event: Event) -> bool {
        let mut pending = PendingChanges::default();
        pending.record(&event);
        let changes = pending.take(&event.paths);
        self.coverage
            .reconcile(&mut self.backend, &mut self.active, &self.specs, &changes)
            .unwrap()
    }

    fn modify(&mut self, path: PathBuf) -> bool {
        self.apply(Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Any))).add_path(path))
    }
}

fn builds() -> usize {
    crate::ignore_plan::BUILDS.with(|count| count.get())
}

#[test]
fn git_churn_rebuild_count_drops_without_losing_index_observation() {
    let mut fixture = Fixture::new();
    let index_path = fixture.repo.path().join("index");
    let lock_path = fixture.repo.path().join("index.lock");
    fixture.repo.index().unwrap().write().unwrap();
    let before = builds();
    for _ in 0..32 {
        fixture.repo.index().unwrap().write().unwrap();
        let signals = crate::backend::refresh_coverage(
            &mut fixture.backend,
            &mut fixture.active,
            &fixture.specs,
            &[index_path.clone()],
            false,
        );
        assert!(signals.is_empty());
        std::fs::write(&lock_path, "temporary").unwrap();
        std::fs::remove_file(&lock_path).unwrap();
        let signals = crate::backend::refresh_coverage(
            &mut fixture.backend,
            &mut fixture.active,
            &fixture.specs,
            &[lock_path.clone()],
            true,
        );
        assert!(signals.is_empty());
        let signals = crate::backend::refresh_coverage(
            &mut fixture.backend,
            &mut fixture.active,
            &fixture.specs,
            &[lock_path.clone()],
            false,
        );
        assert!(signals.is_empty());
    }
    let legacy = builds() - before;
    let before = builds();
    for _ in 0..32 {
        fixture.repo.index().unwrap().write().unwrap();
        assert!(!fixture.modify(index_path.clone()));
        std::fs::write(&lock_path, "temporary").unwrap();
        std::fs::remove_file(&lock_path).unwrap();
        assert!(!fixture
            .apply(Event::new(EventKind::Remove(RemoveKind::File)).add_path(lock_path.clone())));
    }
    let cached = builds() - before;
    eprintln!("git-churn rounds=32 legacy_rebuilds={legacy} cached_rebuilds={cached}");
    assert_eq!(legacy, 128);
    assert_eq!(cached, 0);
}

#[test]
fn new_tracked_file_in_ignored_directory_gains_coverage() {
    let mut fixture = Fixture::new();
    let owned = fixture.path("ignored/deep/owned.txt");
    assert!(!fixture.active.contains_key(owned.parent().unwrap()));
    std::fs::write(&owned, "owned").unwrap();
    let mut index = fixture.repo.index().unwrap();
    index.add_path(Path::new("ignored/deep/owned.txt")).unwrap();
    index.write().unwrap();
    let before = builds();
    assert!(fixture.modify(fixture.repo.path().join("index")));
    assert_eq!(builds() - before, 1);
    assert!(fixture.active.contains_key(owned.parent().unwrap()));
    assert!(!fixture
        .active
        .contains_key(&fixture.path("ignored/unowned")));
}

#[test]
fn ignore_edit_rebuilds_once_and_changes_subscriptions() {
    let mut fixture = Fixture::new();
    std::fs::write(fixture.path(".gitignore"), "ignored/\nsource/\n").unwrap();
    let before = builds();
    assert!(fixture.modify(fixture.path(".gitignore")));
    assert_eq!(builds() - before, 1);
    assert!(!fixture.active.contains_key(&fixture.path("source")));
}

#[test]
fn directory_replacement_rebuilds_once_and_rearms_unchanged_paths() {
    let mut fixture = Fixture::new();
    let source = fixture.path("source");
    std::fs::remove_dir(&source).unwrap();
    std::fs::create_dir(&source).unwrap();
    let before = builds();
    assert!(
        fixture.apply(Event::new(EventKind::Remove(RemoveKind::Folder)).add_path(source.clone()))
    );
    assert_eq!(builds() - before, 1);
    assert_eq!(
        fixture
            .backend
            .started
            .iter()
            .filter(|path| **path == source)
            .count(),
        2
    );
    assert!(fixture.backend.stopped.contains(&source));
    assert_eq!(fixture.active.get(&source), Some(&true));
}

#[test]
fn ordinary_file_delete_and_rename_keep_cached_coverage() {
    let mut fixture = Fixture::new();
    let deleted = fixture.path("deleted.txt");
    let old = fixture.path("old.txt");
    let new = fixture.path("new.txt");
    std::fs::write(&deleted, "deleted").unwrap();
    std::fs::write(&old, "renamed").unwrap();
    let before = builds();
    std::fs::remove_file(&deleted).unwrap();
    assert!(!fixture.apply(Event::new(EventKind::Remove(RemoveKind::File)).add_path(deleted)));
    std::fs::rename(&old, &new).unwrap();
    assert!(!fixture.apply(
        Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
            .add_path(old.clone())
            .add_path(new)
    ));
    assert!(!fixture
        .apply(Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::From))).add_path(old)));
    assert_eq!(builds() - before, 0);
}

#[test]
fn coalesced_directory_replacement_survives_later_content_events() {
    let mut fixture = Fixture::new();
    let source = fixture.path("source");
    let mut pending = PendingChanges::default();
    pending.record(&Event::new(EventKind::Remove(RemoveKind::Folder)).add_path(source.clone()));
    pending.record(
        &Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Any))).add_path(source.clone()),
    );
    let paths = vec![source];
    let changes = pending.take(&paths);
    assert!(pending.take(&paths).is_empty());
    let before = builds();
    assert!(fixture
        .coverage
        .reconcile(
            &mut fixture.backend,
            &mut fixture.active,
            &fixture.specs,
            &changes
        )
        .unwrap());
    assert_eq!(builds() - before, 1);
}

#[test]
fn directory_rename_with_path_reused_by_a_file_drops_stale_coverage() {
    let mut fixture = Fixture::new();
    let source = fixture.path("source");
    std::fs::remove_dir(&source).unwrap();
    std::fs::write(&source, "replacement file").unwrap();
    let before = builds();
    assert!(fixture.apply(
        Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::From))).add_path(source.clone())
    ));
    assert_eq!(builds() - before, 1);
    assert!(!fixture.active.contains_key(&source));
}

#[cfg(windows)]
#[test]
fn ignore_edit_matches_windows_event_path_case() {
    let mut fixture = Fixture::new();
    std::fs::write(fixture.path(".gitignore"), "ignored/\nsource/\n").unwrap();
    let before = builds();
    assert!(fixture.modify(fixture.path(".GITIGNORE")));
    assert_eq!(builds() - before, 1);
    assert!(!fixture.active.contains_key(&fixture.path("source")));
}
