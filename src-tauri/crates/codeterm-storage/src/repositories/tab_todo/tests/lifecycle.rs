use super::*;
use crate::repositories::tab_todo::{TabTodoItemRecord, MAX_TAB_TODO_ITEM_EVENTS};

fn open_repository() -> (TempRoot, Storage, TabTodoRepository) {
    let root = TempRoot::new();
    let storage = Storage::open(root.path(), StorageConfig::default()).expect("open storage");
    let repository = storage.tab_todos();
    (root, storage, repository)
}

fn seed(repository: &TabTodoRepository, tab_id: &str, ids: &[&str]) -> TabTodoChecklistRecord {
    wait(
        repository.create(
            tab_id.into(),
            "workspace".into(),
            Some("team".into()),
            100,
            ids.iter()
                .map(|id| item(id, &format!("Step {id}")))
                .collect(),
            vec![],
        ),
    )
    .value
}

fn live<'a>(checklist: &'a TabTodoChecklistRecord, id: &str) -> &'a TabTodoItemRecord {
    checklist
        .items
        .iter()
        .find(|item| item.id == id)
        .expect("live item")
}

fn move_to(
    repository: &TabTodoRepository,
    checklist: &TabTodoChecklistRecord,
    id: &str,
    state: &str,
    reason: Option<&str>,
    at: u64,
) -> Result<TabTodoChecklistRecord, StorageError> {
    outcome(repository.set_state(
        checklist.tab_id.clone(),
        checklist.generation,
        id.into(),
        live(checklist, id).revision,
        state.into(),
        reason.map(str::to_owned),
        Some(checklist.tab_id.clone()),
        at,
    ))
    .map(|committed| committed.value)
}

fn shutdown(storage: Storage) {
    storage
        .shutdown()
        .wait(Duration::from_secs(5))
        .expect("shutdown storage");
}

#[test]
fn restarting_a_completed_item_keeps_its_id_and_counts_the_attempt() {
    let (_root, storage, repository) = open_repository();
    let list = seed(&repository, "tab-r", &["build"]);
    let list = move_to(&repository, &list, "build", "in_progress", None, 110).unwrap();
    let list = move_to(&repository, &list, "build", "done", None, 150).unwrap();
    assert_eq!(list.status, "complete");
    let list = move_to(
        &repository,
        &list,
        "build",
        "in_progress",
        Some("QA found a regression"),
        200,
    )
    .unwrap();
    let step = live(&list, "build");
    assert_eq!(list.items.len(), 1);
    assert_eq!(step.attempt, 2);
    assert_eq!(step.reason.as_deref(), Some("QA found a regression"));
    assert_eq!(step.counters.started, 2);
    assert_eq!(step.counters.completed, 1);
    assert_eq!(list.status, "active");
    let kinds: Vec<_> = step
        .events
        .iter()
        .map(|event| (event.kind.as_str(), event.attempt, event.happened_at))
        .collect();
    assert_eq!(
        kinds,
        [
            ("started", 1, 110),
            ("completed", 1, 150),
            ("started", 2, 200)
        ]
    );
    assert_eq!(step.events[2].actor_tab_id.as_deref(), Some("tab-r"));
    assert_eq!(
        step.events[2].reason.as_deref(),
        Some("QA found a regression")
    );
    shutdown(storage);
}

#[test]
fn per_item_history_keeps_the_newest_events_and_folds_older_ones_into_counters() {
    let (_root, storage, repository) = open_repository();
    let mut list = seed(&repository, "tab-h", &["loop", "other"]);
    let rounds = MAX_TAB_TODO_ITEM_EVENTS as u64;
    for round in 0..rounds {
        list = move_to(
            &repository,
            &list,
            "loop",
            "in_progress",
            None,
            1000 + round * 2,
        )
        .unwrap();
        list = move_to(&repository, &list, "loop", "done", None, 1001 + round * 2).unwrap();
    }
    let step = live(&list, "loop");
    assert_eq!(step.events.len(), MAX_TAB_TODO_ITEM_EVENTS);
    assert_eq!(step.counters.started, rounds);
    assert_eq!(step.counters.completed, rounds);
    assert_eq!(
        step.counters.folded_events,
        rounds * 2 - MAX_TAB_TODO_ITEM_EVENTS as u64
    );
    assert_eq!(step.attempt, rounds);
    assert_eq!(
        step.events.last().unwrap().happened_at,
        1001 + (rounds - 1) * 2
    );
    assert!(live(&list, "other").events.is_empty());
    shutdown(storage);
}

#[test]
fn blocked_and_skipped_require_reasons_and_skipped_items_close_the_list() {
    let (_root, storage, repository) = open_repository();
    let list = seed(&repository, "tab-b", &["gate", "docs"]);
    assert!(matches!(
        move_to(&repository, &list, "gate", "blocked", None, 110),
        Err(StorageError::Invalid { .. })
    ));
    assert!(matches!(
        move_to(&repository, &list, "docs", "skipped", Some("  "), 110),
        Err(StorageError::Invalid { .. })
    ));
    let list = move_to(
        &repository,
        &list,
        "gate",
        "blocked",
        Some("waiting on the canonical gate"),
        120,
    )
    .unwrap();
    assert_eq!(live(&list, "gate").state, "blocked");
    assert_eq!(live(&list, "gate").counters.blocked, 1);
    let list = move_to(&repository, &list, "gate", "done", None, 130).unwrap();
    let list = move_to(
        &repository,
        &list,
        "docs",
        "skipped",
        Some("owner moved docs to a follow-up"),
        140,
    )
    .unwrap();
    assert_eq!(list.status, "complete");
    let docs = live(&list, "docs");
    assert_eq!(docs.state, "skipped");
    assert_eq!(
        docs.reason.as_deref(),
        Some("owner moved docs to a follow-up")
    );
    assert_eq!(docs.events[0].kind, "skipped");
    shutdown(storage);
}

#[test]
fn locked_items_are_never_deleted_by_clear_or_reset_but_can_be_skipped() {
    let (_root, storage, repository) = open_repository();
    let list = seed(&repository, "tab-l", &["locked"]);
    assert!(live(&list, "locked").locked);
    let refused = [
        outcome(repository.clear_one(
            "tab-l".into(),
            1,
            "locked".into(),
            live(&list, "locked").revision,
            None,
            110,
        ))
        .map(|_| ()),
        outcome(repository.clear_all("tab-l".into(), 1, None, 111)).map(|_| ()),
        outcome(repository.reset(
            "tab-l".into(),
            1,
            "workspace".into(),
            Some("team".into()),
            112,
            None,
            vec![item("fresh", "Fresh")],
        ))
        .map(|_| ()),
    ];
    for result in refused {
        assert!(matches!(result, Err(StorageError::Invalid { .. })));
    }
    let skipped = move_to(
        &repository,
        &list,
        "locked",
        "skipped",
        Some("covered by a sibling task"),
        120,
    )
    .unwrap();
    assert_eq!(skipped.items.len(), 1);
    assert_eq!(live(&skipped, "locked").state, "skipped");
    shutdown(storage);
}

#[test]
fn reviewer_reopen_names_the_failed_step_and_the_next_start_is_a_restart() {
    let (_root, storage, repository) = open_repository();
    let list = seed(&repository, "tab-o", &["check"]);
    let list = move_to(&repository, &list, "check", "in_progress", None, 110).unwrap();
    let list = move_to(&repository, &list, "check", "done", None, 120).unwrap();
    let reopened = wait(repository.reopen(
        "tab-o".into(),
        list.generation,
        "check".into(),
        "QA rejected: focused selector fails".into(),
        Some("qa-tab".into()),
        130,
    ))
    .value;
    let step = live(&reopened, "check");
    assert_eq!(step.state, "open");
    assert_eq!(
        step.reason.as_deref(),
        Some("QA rejected: focused selector fails")
    );
    assert_eq!(step.counters.reopened, 1);
    assert_eq!(
        step.events.last().unwrap().actor_tab_id.as_deref(),
        Some("qa-tab")
    );
    assert_eq!(reopened.status, "active");
    assert!(matches!(
        outcome(repository.reopen("tab-o".into(), 1, "missing".into(), "x".into(), None, 131)),
        Err(StorageError::NotFound { .. })
    ));
    let restarted = move_to(&repository, &reopened, "check", "in_progress", None, 140).unwrap();
    assert_eq!(live(&restarted, "check").attempt, 2);
    shutdown(storage);
}

#[test]
fn carry_over_moves_items_attempts_and_history_to_the_replacement_tab() {
    let (_root, storage, repository) = open_repository();
    let list = seed(&repository, "old-tab", &["one", "two"]);
    let list = move_to(&repository, &list, "one", "in_progress", None, 110).unwrap();
    let list = move_to(&repository, &list, "one", "done", None, 120).unwrap();
    let carried = wait(repository.transfer(
        "old-tab".into(),
        "new-tab".into(),
        "workspace-2".into(),
        Some("team".into()),
        Some("orchestrator".into()),
        200,
    ))
    .value
    .expect("source checklist carried");
    assert_eq!(carried.tab_id, "new-tab");
    assert_eq!(carried.workspace_id, "workspace-2");
    assert_eq!(carried.generation, list.generation);
    let ids: Vec<_> = carried.items.iter().map(|item| item.id.as_str()).collect();
    assert_eq!(ids, ["one", "two"]);
    assert_eq!(live(&carried, "one").attempt, 1);
    assert_eq!(live(&carried, "one").events.len(), 2);
    assert!(wait(repository.get("old-tab".into())).value.is_none());
    let restarted = move_to(
        &repository,
        &carried,
        "one",
        "in_progress",
        Some("respawned"),
        210,
    )
    .unwrap();
    assert_eq!(live(&restarted, "one").attempt, 2);
    assert!(wait(repository.transfer(
        "missing".into(),
        "fresh".into(),
        "workspace".into(),
        None,
        None,
        220,
    ))
    .value
    .is_none());
    seed(&repository, "occupied", &["x"]);
    assert!(matches!(
        outcome(repository.transfer(
            "new-tab".into(),
            "occupied".into(),
            "workspace".into(),
            None,
            None,
            230,
        )),
        Err(StorageError::Invalid { .. })
    ));
    shutdown(storage);
}

mod proposal;
