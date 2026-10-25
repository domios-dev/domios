use super::*;
use crate::{StorageConfig, StorageError};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "codeterm-storage-tab-todo-{}-{}",
            std::process::id(),
            NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        crate::test_support::seed_empty_root(&path);
        TempRoot(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn wait<T>(completion: Completion<T>) -> T {
    outcome(completion).expect("storage operation")
}

fn outcome<T>(completion: Completion<T>) -> Result<T, StorageError> {
    completion.wait(Duration::from_secs(5))
}

fn item(id: &str, title: &str) -> TabTodoNewItemRecord {
    TabTodoNewItemRecord {
        id: id.into(),
        title: title.into(),
        description: None,
        link_json: None,
    }
}

#[test]
fn todo_rows_commit_incrementally_and_refuse_stale_item_revisions() {
    let root = TempRoot::new();
    let storage = Storage::open(root.path(), StorageConfig::default()).expect("open storage");
    let repository = storage.tab_todos();
    let created = wait(repository.create(
        "tab-a".into(),
        "workspace-a".into(),
        Some("team-a".into()),
        100,
        vec![item("item-a", "First")],
        vec![],
    ));
    assert_eq!(created.value.generation, 1);
    let changed = wait(repository.set_state(
        "tab-a".into(),
        1,
        "item-a".into(),
        1,
        "done".into(),
        None,
        Some("tab-a".into()),
        200,
    ));
    assert_eq!(changed.value.items[0].revision, 2);
    let stale = outcome(repository.set_state(
        "tab-a".into(),
        1,
        "item-a".into(),
        1,
        "open".into(),
        None,
        Some("tab-a".into()),
        201,
    ));
    assert!(matches!(stale, Err(StorageError::StaleRevision { .. })));
    let read = wait(repository.get("tab-a".into()));
    assert_eq!(read.value.expect("list").items[0].state, "done");
    storage
        .shutdown()
        .wait(Duration::from_secs(5))
        .expect("shutdown storage");
}

#[test]
fn clearing_an_item_keeps_tombstone_fence_and_qa_append_order() {
    let root = TempRoot::new();
    let storage = Storage::open(root.path(), StorageConfig::default()).expect("open storage");
    let repository = storage.tab_todos();
    let started = wait(repository.reset(
        "tab-c".into(),
        0,
        "workspace-c".into(),
        Some("team-c".into()),
        100,
        None,
        vec![item("first", "First"), item("second", "Second")],
    ));
    assert_eq!(started.value.generation, 1);
    assert!(started.value.items.iter().all(|item| !item.locked));
    wait(repository.set_state(
        "tab-c".into(),
        1,
        "second".into(),
        1,
        "done".into(),
        None,
        None,
        150,
    ));
    let cleared = wait(repository.clear_one("tab-c".into(), 1, "first".into(), 1, None, 200));
    assert_eq!(cleared.value.items[0].id, "second");
    assert_eq!(cleared.value.items[0].position, 0);
    assert!(matches!(
        outcome(repository.set_state(
            "tab-c".into(),
            1,
            "first".into(),
            1,
            "done".into(),
            None,
            None,
            201
        )),
        Err(StorageError::NotFound { .. })
    ));
    let appended = wait(repository.append(
        "tab-c".into(),
        1,
        Some("qa-tab".into()),
        202,
        vec![item("qa-check", "QA check")],
    ));
    assert_eq!(
        appended
            .value
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        vec!["second", "qa-check"]
    );
    storage
        .shutdown()
        .wait(Duration::from_secs(5))
        .expect("shutdown storage");
}

#[test]
fn todo_reset_fences_old_generation_and_removes_old_items() {
    let root = TempRoot::new();
    let storage = Storage::open(root.path(), StorageConfig::default()).expect("open storage");
    let repository = storage.tab_todos();
    wait(repository.reset(
        "tab-b".into(),
        0,
        "workspace-b".into(),
        None,
        100,
        None,
        vec![item("old", "Old")],
    ));
    let reset = wait(repository.reset(
        "tab-b".into(),
        1,
        "workspace-b".into(),
        None,
        200,
        None,
        vec![item("new", "New")],
    ));
    assert_eq!(reset.value.generation, 2);
    assert_eq!(reset.value.items[0].id, "new");
    assert!(matches!(
        outcome(repository.set_state(
            "tab-b".into(),
            1,
            "old".into(),
            1,
            "done".into(),
            None,
            None,
            300
        )),
        Err(StorageError::StaleRevision { .. })
    ));
    wait(repository.forget_tab("tab-b".into()));
    assert!(wait(repository.get("tab-b".into())).value.is_none());
    storage
        .shutdown()
        .wait(Duration::from_secs(5))
        .expect("shutdown storage");
}

#[test]
fn workspace_and_team_cleanup_preserve_a_tab_generation_moved_elsewhere() {
    let root = TempRoot::new();
    let storage = Storage::open(root.path(), StorageConfig::default()).expect("open storage");
    let repository = storage.tab_todos();
    wait(repository.reset(
        "tab-d".into(),
        0,
        "workspace-old".into(),
        Some("team-old".into()),
        100,
        None,
        vec![item("old", "Old")],
    ));
    wait(repository.reset(
        "tab-d".into(),
        1,
        "workspace-new".into(),
        Some("team-new".into()),
        200,
        None,
        vec![item("new", "New")],
    ));
    wait(repository.forget_workspace("workspace-old".into()));
    let current = wait(repository.get("tab-d".into()))
        .value
        .expect("new workspace generation remains");
    assert_eq!(current.generation, 2);
    assert_eq!(current.items[0].id, "new");
    wait(repository.forget_team("team-old".into()));
    assert_eq!(
        wait(repository.get("tab-d".into()))
            .value
            .unwrap()
            .generation,
        2
    );
    wait(repository.forget_team("team-new".into()));
    assert!(wait(repository.get("tab-d".into())).value.is_none());
    storage
        .shutdown()
        .wait(Duration::from_secs(5))
        .expect("shutdown storage");
}

mod lifecycle;
