use std::sync::{mpsc, Arc, Barrier, Mutex as StdMutex};
use std::time::Duration;

use crate::plan::OrchestratorRef;
use crate::service_plan::PlanStore;

fn store() -> (tempfile::TempDir, Arc<PlanStore>, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(PlanStore::new_for_test(dir.path()));
    let plan = store
        .create_plan(
            "Initial".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_pane("orch"),
            vec![],
            1,
        )
        .unwrap();
    (dir, store, plan.id)
}

#[test]
fn writer_queue_serializes_persistence_in_submission_order() {
    let (_dir, store, plan_id) = store();
    let (first_started_tx, first_started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Arc::new(StdMutex::new(Some(release_rx)));
    let persisted = Arc::new(StdMutex::new(Vec::new()));
    store.set_persistence_hook_for_test(Some(Arc::new({
        let release_rx = release_rx.clone();
        let persisted = persisted.clone();
        move |plan| {
            if plan.title == "First" {
                first_started_tx.send(()).unwrap();
                release_rx.lock().unwrap().take().unwrap().recv().unwrap();
            }
            persisted.lock().unwrap().push(plan.title.clone());
        }
    })));

    let first_store = store.clone();
    let first_id = plan_id.clone();
    let first = std::thread::spawn(move || {
        first_store.update_plan(&first_id, Some("First"), None, None, 2)
    });
    first_started_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap();

    let (second_attempt_tx, second_attempt_rx) = mpsc::channel();
    let second_store = store.clone();
    let second_id = plan_id.clone();
    let second = std::thread::spawn(move || {
        second_attempt_tx.send(()).unwrap();
        second_store.update_plan(&second_id, Some("Second"), None, None, 3)
    });
    second_attempt_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
    assert!(persisted.lock().unwrap().is_empty());

    release_tx.send(()).unwrap();
    first.join().unwrap().unwrap();
    second.join().unwrap().unwrap();
    assert_eq!(&*persisted.lock().unwrap(), &["First", "Second"]);
    assert_eq!(store.get(&plan_id).unwrap().title, "Second");
}

#[test]
fn reads_keep_last_committed_snapshot_while_persistence_is_blocked() {
    let (_dir, store, plan_id) = store();
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Arc::new(StdMutex::new(Some(release_rx)));
    store.set_persistence_hook_for_test(Some(Arc::new({
        let release_rx = release_rx.clone();
        move |plan| {
            if plan.title == "Blocked" {
                started_tx.send(()).unwrap();
                release_rx.lock().unwrap().take().unwrap().recv().unwrap();
            }
        }
    })));

    let writer_store = store.clone();
    let writer_id = plan_id.clone();
    let writer = std::thread::spawn(move || {
        writer_store.update_plan(&writer_id, Some("Blocked"), None, None, 2)
    });
    started_rx.recv_timeout(Duration::from_secs(1)).unwrap();

    let (read_tx, read_rx) = mpsc::channel();
    let reader_store = store.clone();
    let reader_id = plan_id.clone();
    std::thread::spawn(move || {
        read_tx
            .send(reader_store.get(&reader_id).unwrap().title)
            .unwrap()
    });
    assert_eq!(
        read_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        "Initial"
    );

    release_tx.send(()).unwrap();
    writer.join().unwrap().unwrap();
    assert_eq!(store.get(&plan_id).unwrap().title, "Blocked");
}

#[test]
fn concurrent_team_plan_writes_all_commit_through_one_queue() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(PlanStore::new_for_test(dir.path()));
    let workers = 8;
    let barrier = Arc::new(Barrier::new(workers));
    let mut joins = Vec::new();
    for number in 0..workers {
        let store = store.clone();
        let barrier = barrier.clone();
        let project_path = dir.path().display().to_string();
        joins.push(std::thread::spawn(move || {
            barrier.wait();
            store.create_plan_for_team(
                format!("Concurrent {number}"),
                project_path,
                OrchestratorRef {
                    pane_id: Some(format!("orchestrator-{number}")),
                    ..Default::default()
                },
                format!("team-{number}"),
                vec![],
                number as u64,
            )
        }));
    }

    let plans = joins
        .into_iter()
        .map(|join| join.join().unwrap().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(plans.len(), workers);
    assert_eq!(store.list_full().len(), workers);
    assert_eq!(store.operation_count_for_test(None), workers);
}
