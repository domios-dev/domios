use super::*;
use crate::store::Owner;
use codeterm_service_core::cron::clock::MockClock;
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::service::test_fixture::ServiceFixture;
use std::sync::mpsc;
use std::time::Instant;

fn watch(store: &mut Store, owner: &Owner, name: &str) -> Sentinel {
    let document = codeterm_sentinel::test_support::watch(
        json!(name),
        json!({"uses":"probe","with":{"command":{"lit":name}},"every":{"lit":"10s"},"timeout":{"lit":"1h"}}),
        json!({"with":{"question":{"lit":"state"}},"buckets":["done"],"matches":{"done":{"lit":"^done$"}}}),
        json!({"done": []}),
        serde_json::Value::Null,
    );
    store
        .create_document(document, owner.clone(), false)
        .unwrap()
}

#[test]
fn blueprint_v2_budget_stalled_probe_leaves_scheduler_and_other_verdicts_running() {
    let (mux, creator) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&creator).unwrap();
    let fixture = ServiceFixture::new(mux);
    let clock = MockClock::starting_at(1000);
    let mut store = Store::new(fixture.temp.path().join("budget.json"), clock.clone());
    let owner = Owner {
        creator_tab_id: creator,
        team_id: None,
        workspace_id: workspace,
        cwd: fixture.temp.path().to_string_lossy().into_owned(),
    };
    let slow = watch(&mut store, &owner, "stalled");
    let fast = watch(&mut store, &owner, "fast");
    let handle = fixture_handle_without_app(store);
    let leases = storage::commit(&handle.store, Store::stage_due).unwrap();
    assert_eq!(leases.len(), 2);
    let (started_tx, started_rx) = mpsc::channel();
    let (expire_tx, expire_rx) = mpsc::channel();
    let expire_rx = Arc::new(std::sync::Mutex::new(expire_rx));
    let (done_tx, done_rx) = mpsc::channel();
    let slow_id = slow.id.clone();
    let workers = dispatch_leases(
        &handle,
        &fixture.service,
        leases,
        move |handle, svc, lease| {
            tick_with_probe(handle, svc, lease, |limit, runtime| {
                assert_eq!(runtime, Duration::from_secs(10));
                assert!(limit > runtime);
                let at = Instant::now();
                let mut budget = probe::process::Deadlines::at(limit, runtime, at).unwrap();
                budget.spawn_with(|| "fake-child", || at);
                if lease.job.id == slow_id {
                    started_tx.send((at, budget.operation)).unwrap();
                    let expired_at = expire_rx
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(5))
                        .unwrap();
                    assert!(!budget.expired_operation_at(expired_at - Duration::from_nanos(1)));
                    assert!(budget.expired_operation_at(expired_at));
                    let failure = probe::process::ProbeFailure::new("probe_timeout");
                    assert!(failure.is_deadline());
                    Err(failure)
                } else {
                    Ok(probe::fixture_output(0, b"done", b""))
                }
            });
            done_tx.send(lease.job.id.clone()).unwrap();
        },
    );
    let (at, deadline) = started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(deadline.duration_since(at), Duration::from_secs(10));
    assert_eq!(
        done_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        fast.id
    );
    assert_eq!(
        handle
            .store
            .lock()
            .get(&fast.id)
            .unwrap()
            .machine
            .last_bucket
            .as_deref(),
        Some("done")
    );
    assert_eq!(
        handle
            .store
            .lock()
            .get(&slow.id)
            .unwrap()
            .machine
            .last_bucket,
        None
    );
    clock.advance_ms(1);
    let third = watch(&mut handle.store.lock(), &owner, "next-tick");
    let next = storage::commit(&handle.store, Store::stage_due).unwrap();
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].job.id, third.id);
    let (tick_tx, tick_rx) = mpsc::channel();
    let next_workers = dispatch_leases(
        &handle,
        &fixture.service,
        next,
        move |handle, svc, lease| {
            tick_with_probe(handle, svc, lease, |_, runtime| {
                assert_eq!(runtime, Duration::from_secs(10));
                Ok(probe::fixture_output(0, b"done", b""))
            });
            tick_tx.send(()).unwrap();
        },
    );
    tick_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
        handle
            .store
            .lock()
            .get(&third.id)
            .unwrap()
            .machine
            .last_bucket
            .as_deref(),
        Some("done")
    );
    clock.advance_ms(9_999);
    expire_tx.send(deadline).unwrap();
    assert_eq!(
        done_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        slow.id
    );
    for worker in workers.into_iter().chain(next_workers) {
        worker.join().unwrap();
    }
    assert!(handle
        .store
        .lock()
        .get(&slow.id)
        .unwrap()
        .last_reason
        .as_deref()
        .unwrap()
        .starts_with("probe_timeout"));
}
