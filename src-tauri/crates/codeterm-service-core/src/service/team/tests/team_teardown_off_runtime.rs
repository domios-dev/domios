use std::future::Future;
use std::sync::mpsc;
use std::task::Poll;
use std::time::Duration;

use crate::cron::clock::MockClock;
use crate::sentinel::scheduler;
use crate::sentinel::store::{Create, Owner, Store};
use crate::service::test_fixture::ServiceFixture;
use crate::storage::{Storage, StorageConfig};
use tauri::Manager;

const DEADLINE: Duration = Duration::from_secs(30);

#[test]
fn team_teardown_off_runtime_holds_sentinel_writer_without_stalling_runtime() {
    let (mux, team_id, orchestrator, _, _) = super::team_members();
    let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
    let fixture = ServiceFixture::new(mux);
    let storage = Storage::open(fixture.temp.path(), StorageConfig::default()).unwrap();
    let store = Store::with_storage(&storage, MockClock::starting_at(1_000));
    let sentinels = scheduler::fixture_handle(store, fixture._app.handle().clone());
    let job = sentinels
        .store
        .lock()
        .prepare(
            Create {
                terminal: None,
                session: false,
                name: Some("Team teardown barrier".into()),
                probe: Some("status".into()),
                observe: None,
                input: None,
                question: "Did the state change?".into(),
                buckets: vec!["waiting".into(), "passed".into(), "failed".into()],
                every: "20s".into(),
                routes: Default::default(),
                matches: [("waiting".to_string(), ".*".to_string())]
                    .into_iter()
                    .collect(),
                rearm_on_change: Default::default(),
                until: None,
                timeout: Some("1h".into()),
                paused: true,
                handlers: None,
                model_classification: false,
                escalation_timeout_ms: None,
                max_transitions: None,
                max_actions: None,
            },
            Owner {
                creator_tab_id: orchestrator,
                team_id: Some(team_id.clone()),
                workspace_id,
                cwd: fixture.temp.path().display().to_string(),
            },
            false,
        )
        .unwrap();
    let sentinel =
        crate::sentinel::storage::commit(&sentinels.store, |current| current.stage_insert(job))
            .unwrap();
    let sentinel_id = sentinel.id;
    assert!(fixture._app.manage(sentinels.clone()));
    crate::sentinel::manage_fixture_ports(&fixture._app.handle().clone(), &sentinels);

    let hold = storage.hold_writer().unwrap();
    hold.wait_held(DEADLINE).unwrap();
    let committed_before = storage.metrics().committed_operations;
    let service = fixture.service.clone();
    let runtime_team_id = team_id.clone();
    let (unrelated_tx, unrelated_rx) = mpsc::channel();
    let (settled_tx, settled_rx) = mpsc::channel();
    let runtime = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let mut teardown = Box::pin(super::finish_team_teardown(&service, &runtime_team_id));
            let waiting_on_storage =
                std::future::poll_fn(|cx| Poll::Ready(teardown.as_mut().poll(cx).is_pending()))
                    .await;
            assert!(
                waiting_on_storage,
                "teardown must await its blocking worker"
            );

            tokio::spawn(async move {
                unrelated_tx.send(()).expect("runtime observer is waiting");
            })
            .await
            .unwrap();

            let still_waiting_on_storage =
                std::future::poll_fn(|cx| Poll::Ready(teardown.as_mut().poll(cx).is_pending()))
                    .await;
            settled_tx
                .send(still_waiting_on_storage)
                .expect("test is waiting for the held-writer state");
            teardown.await
        })
    });

    if let Err(error) = unrelated_rx.recv_timeout(DEADLINE) {
        hold.release();
        let _ = runtime.join();
        panic!("unrelated runtime work stalled behind Sentinel Storage: {error}");
    }
    assert!(sentinels.store.lock().get(&sentinel_id).is_some());
    let still_waiting_on_storage = settled_rx.recv_timeout(DEADLINE);
    hold.release();
    let teardown_result = runtime.join();
    assert!(
        still_waiting_on_storage.is_ok_and(|waiting| waiting),
        "Sentinel removal must remain unsettled while the writer is held"
    );

    teardown_result.unwrap().unwrap();
    assert_eq!(
        storage.metrics().committed_operations,
        committed_before + 1,
        "one Sentinel removal must settle once"
    );
    assert!(sentinels.store.lock().get(&sentinel_id).is_none());
    assert!(!storage
        .sentinels()
        .inventory()
        .wait(DEADLINE)
        .unwrap()
        .value
        .iter()
        .any(|stored| stored.record.identity.id.as_str() == sentinel_id.as_str()));
    storage.shutdown().wait(DEADLINE).unwrap();
}
