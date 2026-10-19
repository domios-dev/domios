use super::*;
use crate::{StorageConfig, StorageError};
use std::time::Duration;

fn wait<T>(completion: Completion<T>) -> T {
    completion
        .wait(Duration::from_secs(5))
        .expect("storage operation")
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
    let root = tempfile::tempdir().expect("storage root");
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
        Some("tab-a".into()),
        200,
    ));
    assert_eq!(changed.value.items[0].revision, 2);
    let stale = wait(repository.set_state(
        "tab-a".into(),
        1,
        "item-a".into(),
        1,
        "open".into(),
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
    let root = tempfile::tempdir().expect("storage root");
    let storage = Storage::open(root.path(), StorageConfig::default()).expect("open storage");
    let repository = storage.tab_todos();
    wait(repository.create(
        "tab-c".into(),
        "workspace-c".into(),
        Some("team-c".into()),
        100,
        vec![item("first", "First"), item("second", "Second")],
        vec![],
    ));
    wait(repository.set_state(
        "tab-c".into(),
        1,
        "second".into(),
        1,
        "done".into(),
        None,
        150,
    ));
    let cleared = wait(repository.clear_one("tab-c".into(), 1, "first".into(), 1, None, 200));
    assert_eq!(cleared.value.items[0].id, "second");
    assert_eq!(cleared.value.items[0].position, 0);
    assert!(matches!(
        wait(repository.set_state(
            "tab-c".into(),
            1,
            "first".into(),
            1,
            "done".into(),
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
    let root = tempfile::tempdir().expect("storage root");
    let storage = Storage::open(root.path(), StorageConfig::default()).expect("open storage");
    let repository = storage.tab_todos();
    wait(repository.create(
        "tab-b".into(),
        "workspace-b".into(),
        None,
        100,
        vec![item("old", "Old")],
        vec![],
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
        wait(repository.set_state("tab-b".into(), 1, "old".into(), 1, "done".into(), None, 300)),
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
    let root = tempfile::tempdir().expect("storage root");
    let storage = Storage::open(root.path(), StorageConfig::default()).expect("open storage");
    let repository = storage.tab_todos();
    wait(repository.create(
        "tab-d".into(),
        "workspace-old".into(),
        Some("team-old".into()),
        100,
        vec![item("old", "Old")],
        vec![],
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
