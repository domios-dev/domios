use super::*;
use crate::plan::OrchestratorRef;
use rusqlite::Connection;
use std::collections::BTreeSet;
use std::sync::mpsc;

fn store_with_plans(count: usize) -> (tempfile::TempDir, Arc<PlanStore>, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let ids = (0..count)
        .map(|index| {
            let mut plan = Plan::new(
                format!("Plan {index}"),
                dir.path().display().to_string(),
                OrchestratorRef::from_tab("owner-tab"),
            );
            store.save(&mut plan, 1 + index as u64).unwrap();
            plan.id
        })
        .collect();
    store.clear_plan_cache_for_test();
    store.reset_plan_loads_for_test();
    (dir, Arc::new(store), ids)
}

fn sorted(ids: impl IntoIterator<Item = String>) -> Vec<String> {
    ids.into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[test]
fn open_builds_the_index_without_parsing_any_plan_row() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut good = Plan::new(
        "Good".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("owner-tab"),
    );
    store.save(&mut good, 1).unwrap();
    let database_path = store.database_path_for_test();
    drop(store);

    let connection = Connection::open(&database_path).unwrap();
    connection
        .execute(
            "INSERT INTO plans(id, title, status, team_id, project_path, updated_at, header_json)
             VALUES ('pl_corrupt', 'Corrupt', 'active', NULL, ?1, 2, '\"this is not a plan\"')",
            [dir.path().display().to_string()],
        )
        .unwrap();
    drop(connection);

    let reopened = PlanStore::open(database_path).unwrap();
    let listed: Vec<String> = reopened
        .list_index()
        .into_iter()
        .map(|entry| entry.plan_id)
        .collect();
    assert_eq!(
        sorted(listed),
        sorted([good.id.clone(), "pl_corrupt".into()])
    );
    assert!(
        reopened.plan_loads_for_test().is_empty(),
        "open and list_index must load no plan"
    );
    assert!(reopened.cached_plan_ids_for_test().is_empty());

    assert!(matches!(
        reopened.try_get("pl_corrupt"),
        Err(PlanError::Quarantined { plan_id, .. }) if plan_id == "pl_corrupt"
    ));
    assert!(reopened.get("pl_corrupt").is_none());
    assert_eq!(reopened.get(&good.id).unwrap().title, "Good");
    assert_eq!(
        reopened.plan_loads_for_test(),
        vec![
            "pl_corrupt".to_string(),
            "pl_corrupt".into(),
            good.id.clone()
        ]
    );
    assert_eq!(reopened.cached_plan_ids_for_test(), vec![good.id]);
}

#[test]
fn recent_use_cache_evicts_least_recently_used_beyond_capacity() {
    let (_dir, store, ids) = store_with_plans(PLAN_CACHE_CAPACITY + 2);

    for id in &ids[..PLAN_CACHE_CAPACITY] {
        store.get(id).unwrap();
    }
    assert_eq!(store.cached_plan_ids_for_test().len(), PLAN_CACHE_CAPACITY);
    store.get(&ids[0]).unwrap();
    store.get(&ids[PLAN_CACHE_CAPACITY]).unwrap();
    store.get(&ids[PLAN_CACHE_CAPACITY + 1]).unwrap();

    let expected = sorted(
        ids.iter()
            .enumerate()
            .filter(|(index, _)| *index != 1 && *index != 2)
            .map(|(_, id)| id.clone()),
    );
    assert_eq!(store.cached_plan_ids_for_test(), expected);
    assert_eq!(store.plan_loads_for_test().len(), PLAN_CACHE_CAPACITY + 2);

    store.get(&ids[1]).unwrap();
    assert_eq!(store.plan_loads_for_test().last(), Some(&ids[1]));
    assert_eq!(store.cached_plan_ids_for_test().len(), PLAN_CACHE_CAPACITY);
}

#[test]
fn cache_never_evicts_a_plan_with_a_mutation_in_flight() {
    let (_dir, store, ids) = store_with_plans(PLAN_CACHE_CAPACITY + 2);
    let target = ids[0].clone();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let mutation_store = store.clone();
    let mutation_target = target.clone();
    let mutation = std::thread::spawn(move || {
        mutation_store.with_plan(&mutation_target, 50, |plan| {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            plan.title = "Mutated in flight".into();
            Ok(())
        })
    });
    entered_rx.recv().unwrap();

    for id in &ids[1..] {
        store.get(id).unwrap();
    }
    let cached = store.cached_plan_ids_for_test();
    assert!(cached.contains(&target), "the in-flight plan was evicted");
    assert_eq!(cached.len(), PLAN_CACHE_CAPACITY);

    release_tx.send(()).unwrap();
    mutation.join().unwrap().unwrap();
    assert_eq!(store.get(&target).unwrap().title, "Mutated in flight");
    assert!(store.cached_plan_ids_for_test().contains(&target));
    assert!(store.cached_plan_ids_for_test().len() <= PLAN_CACHE_CAPACITY);
}

#[test]
fn failed_persist_evicts_the_cached_plan_so_memory_matches_disk() {
    let (_dir, store, ids) = store_with_plans(1);
    let id = ids[0].clone();
    let before = store.get(&id).unwrap();
    assert!(store.cached_plan_ids_for_test().contains(&id));

    let connection = Connection::open(store.database_path_for_test()).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER fail_plan_update BEFORE UPDATE ON plans
             BEGIN SELECT RAISE(ABORT, 'injected write failure'); END;",
        )
        .unwrap();
    let failed = store.with_plan(&id, 99, |plan| {
        plan.title = "Never stored".into();
        Ok(())
    });
    assert!(matches!(failed, Err(PlanError::Io(_))), "{failed:?}");
    assert!(
        !store.cached_plan_ids_for_test().contains(&id),
        "a failed write must not leave the plan cached"
    );
    connection
        .execute_batch("DROP TRIGGER fail_plan_update")
        .unwrap();

    store.reset_plan_loads_for_test();
    assert_eq!(store.get(&id).unwrap(), before);
    assert_eq!(store.plan_loads_for_test(), vec![id.clone()]);
    let entry = store
        .list_index()
        .into_iter()
        .find(|entry| entry.plan_id == id)
        .unwrap();
    assert_eq!(entry.title, before.title);
    assert_eq!(store.index.read()[&id].title, before.title);
}

#[test]
fn mutating_an_uncached_plan_loads_locks_writes_and_reloads_equal() {
    let (_dir, store, ids) = store_with_plans(1);
    let id = ids[0].clone();
    assert!(store.cached_plan_ids_for_test().is_empty());

    store
        .with_plan(&id, 9, |plan| {
            plan.title = "Loaded then written".into();
            Ok(())
        })
        .unwrap();
    assert_eq!(store.plan_loads_for_test(), vec![id.clone()]);
    assert_eq!(store.cached_plan_ids_for_test(), vec![id.clone()]);

    let in_memory = store.get(&id).unwrap();
    assert_eq!(
        store.plan_loads_for_test().len(),
        1,
        "the cached copy serves reads"
    );
    assert_eq!(in_memory.title, "Loaded then written");
    assert_eq!(in_memory.updated_at, 9);

    store.clear_plan_cache_for_test();
    assert_eq!(store.get(&id).unwrap(), in_memory);
    assert_eq!(store.plan_loads_for_test().len(), 2);
}

#[test]
fn concurrent_mutations_on_one_plan_serialize_and_both_persist() {
    let (_dir, store, ids) = store_with_plans(1);
    let id = ids[0].clone();
    let order = Arc::new(Mutex::new(Vec::new()));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();

    let first_store = store.clone();
    let first_id = id.clone();
    let first_order = order.clone();
    let first = std::thread::spawn(move || {
        first_store
            .with_plan(&first_id, 10, |plan| {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                plan.title.push_str("-first");
                first_order.lock().push("first");
                Ok(())
            })
            .unwrap();
    });
    entered_rx.recv().unwrap();

    let queued_behind = {
        let state = store.mutation_writer.state.lock();
        state.next_ticket
    };
    let second_store = store.clone();
    let second_id = id.clone();
    let second_order = order.clone();
    let second = std::thread::spawn(move || {
        second_store
            .with_plan(&second_id, 11, |plan| {
                plan.title.push_str("-second");
                second_order.lock().push("second");
                Ok(())
            })
            .unwrap();
    });
    {
        let mut state = store.mutation_writer.state.lock();
        while state.next_ticket <= queued_behind {
            store.mutation_writer.ready.wait(&mut state);
        }
    }
    assert!(
        order.lock().is_empty(),
        "the second mutation ran inside the first one's turn"
    );

    release_tx.send(()).unwrap();
    first.join().unwrap();
    second.join().unwrap();
    assert_eq!(*order.lock(), vec!["first", "second"]);

    store.clear_plan_cache_for_test();
    let stored = store.get(&id).unwrap();
    assert_eq!(stored.title, "Plan 0-first-second");
    assert_eq!(stored.updated_at, 11);
}
