//! Deterministic races against the shared owner: a held writer stands in for a slow commit,
//! so ordering is fixed by barriers and admission order, never by elapsed time.

use super::*;
use codeterm_storage::{Fault, Hold};

fn held(storage: &Storage) -> Hold {
    let hold = storage.hold_writer().expect("writer hold admitted");
    hold.wait_held(DEADLINE).expect("writer reached the hold");
    hold
}

/// A due lease with one accepted transition, ready for claims.
fn accepted(store: &Arc<Mutex<Store>>, handlers: Value) -> (Lease, Sentinel, String) {
    create(store, Some("team"), false, handlers);
    let lease = due(store).remove(0);
    let (job, transition) = accept(store, &lease);
    store.lock().take_changes();
    (lease, job, transition)
}

fn two_handlers() -> Value {
    json!({"done":[
        {"id":"first","type":"notify","parameters":{}},
        {"id":"tail","type":"notify","parameters":{}}
    ]})
}

#[test]
fn storage_sentinel_stop_staged_before_claim_fences_the_effect() {
    let root = temp_root();
    let storage = open(root.path());
    let store = typed(&storage, &MockClock::starting_at(1000));
    let (lease, job, transition) = accepted(&store, two_handlers());
    let handler = job.routes["done"][0].clone();

    let hold = held(&storage);
    let stop = store.lock().stage_stop(&job.id, "external_stop");
    let claim = store
        .lock()
        .stage_claim_handler(&job, &transition, &handler);
    assert!(
        lease.is_cancelled(),
        "the stop fences the lease before its commit"
    );
    {
        let cached = store.lock();
        let current = cached.get(&job.id).unwrap();
        assert!(
            !current.machine.stopped,
            "nothing is published before COMMIT"
        );
        assert!(current.transitions[0].handlers.is_empty());
    }
    assert!(store.lock().take_changes().is_empty());
    hold.release();

    assert!(stop.settle(&store).expect("committed stop").machine.stopped);
    assert!(claim.settle(&store).expect("fenced claim").is_none());
    assert!(stored(&storage, &job.id).record.transitions[0]
        .handlers
        .is_empty());
    let cached = store.lock();
    assert!(cached.get(&job.id).unwrap().machine.stopped);
    assert!(!cached.action_permitted(&job, &transition));
    drop(cached);
    close(storage);
}

#[test]
fn storage_sentinel_team_removal_outlives_late_stop_and_fenced_claim() {
    let root = temp_root();
    let storage = open(root.path());
    let store = typed(&storage, &MockClock::starting_at(1000));
    let (lease, job, transition) = accepted(&store, two_handlers());
    let handler = job.routes["done"][0].clone();

    let hold = held(&storage);
    let stop = store.lock().stage_stop(&job.id, "external_stop");
    let removal = store.lock().stage_remove_team("team");
    let claim = store
        .lock()
        .stage_claim_handler(&job, &transition, &handler);
    hold.release();

    assert_eq!(removal.settle(&store), Ok(()));
    assert!(store.lock().get(&job.id).is_none());
    stop.settle(&store)
        .expect("the stop committed before the removal");
    assert!(
        store.lock().get(&job.id).is_none(),
        "an older outcome never resurrects a removed stage"
    );
    assert!(claim.settle(&store).expect("fenced claim").is_none());
    assert!(lease.is_cancelled());
    assert!(done(storage.sentinels().inventory()).value.is_empty());
    assert_eq!(store.lock().take_changes().len(), 1);
    close(storage);
}

#[test]
fn storage_sentinel_commit_failure_refuses_claim_and_closes_admission() {
    let root = temp_root();
    let storage = open(root.path());
    let store = typed(&storage, &MockClock::starting_at(1000));
    let job = create(&store, Some("team"), false, two_handlers());
    let lease = due(&store).remove(0);
    crate::sentinel::scheduler::advance(
        &store,
        &lease,
        &output("done"),
        || verdict("done"),
        |job, transition, handler| {
            done(storage.inject_fault(Fault::FailNextCommit));
            commit(&store, |current| {
                current.stage_claim_handler(job, transition, handler)
            })
        },
        |_, _, _, _| panic!("a failed durable claim never reaches the effect"),
        |_, _, _| Ok(()),
    );
    assert!(lease.is_cancelled());
    let error = store.lock().ensure_available().unwrap_err();
    assert!(
        error.starts_with("blueprint_store_persistence_failed:"),
        "{error}"
    );
    assert!(commit(&store, Store::stage_due).is_err());
    let durable = stored(&storage, &job.id).record;
    assert_eq!(
        durable.transitions.len(),
        1,
        "the accepted transition committed"
    );
    assert!(
        durable.transitions[0].handlers.is_empty(),
        "no claim, no skip"
    );
    close(storage);
}

#[test]
fn storage_sentinel_unconfirmed_commit_stays_closed_until_reconciled() {
    let root = temp_root();
    let storage = open(root.path());
    let store = typed(&storage, &MockClock::starting_at(1000));
    let (lease, job, transition) = accepted(&store, two_handlers());
    let handler = job.routes["done"][0].clone();

    let hold = held(&storage);
    let claim = store
        .lock()
        .stage_claim_handler(&job, &transition, &handler);
    let error = claim
        .settle_within(&store, Duration::ZERO)
        .expect_err("an expired wait is unconfirmed");
    assert!(error.contains("unconfirmed"), "{error}");
    assert!(lease.is_cancelled());
    assert!(store.lock().ensure_available().is_err());
    assert!(commit(&store, Store::stage_due).is_err());
    hold.release();

    assert_eq!(crate::sentinel::storage::reconcile(&store), Ok(true));
    store
        .lock()
        .ensure_available()
        .expect("reconciled admission");
    let reconciled = store.lock().get(&job.id).cloned().unwrap();
    let claimed = &reconciled.transitions[0].handlers[0];
    assert_eq!(
        claimed.status,
        Status::Unconfirmed,
        "the late commit is adopted"
    );
    assert!(claimed.claim_id.is_some());
    let replay = commit(&store, |current| {
        current.stage_claim_handler(&reconciled, &transition, &handler)
    });
    assert!(
        replay.expect("refusal").is_none(),
        "the unconfirmed claim is never replayed"
    );
    close(storage);
}

#[test]
fn storage_sentinel_held_writer_serves_committed_scope_and_holds_events() {
    let root = temp_root();
    let storage = open(root.path());
    let store = typed(&storage, &MockClock::starting_at(1000));
    let first = create(&store, None, true, json!({}));
    create(&store, None, true, json!({}));
    create(&store, Some("team"), true, json!({}));
    store.lock().take_changes();

    let hold = held(&storage);
    let (scoped, leases) = crate::sentinel::storage::committed_scope(&store, "workspace", None)
        .expect("committed read");
    assert_eq!(scoped.len(), 2, "the exact null-Team scope only");
    assert!(leases.is_empty());
    let staged = store.lock().stage_stop(&first.id, "explicit");
    let settling = {
        let store = store.clone();
        std::thread::spawn(move || staged.settle(&store))
    };
    let unrelated = store.lock().list().len();
    assert_eq!(
        unrelated, 3,
        "the store lock stays free while a commit waits"
    );
    assert!(!store.lock().get(&first.id).unwrap().machine.stopped);
    assert!(
        store.lock().take_changes().is_empty(),
        "no event before COMMIT"
    );
    hold.release();

    let stopped = settling.join().expect("settling thread").expect("stop");
    assert!(stopped.machine.stopped);
    let changes = store.lock().take_changes();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0]["team_id"], Value::Null);
    let committed = store
        .lock()
        .committed_revision("workspace", None)
        .map(str::to_owned);
    assert_eq!(changes[0]["revision"].as_str(), committed.as_deref());
    let (scoped, _) = crate::sentinel::storage::committed_scope(&store, "workspace", None).unwrap();
    assert!(scoped
        .iter()
        .any(|job| job.id == first.id && job.machine.stopped));
    close(storage);
}

#[test]
fn storage_sentinel_shutdown_settles_accepted_work_and_refuses_new_admission() {
    let root = temp_root();
    let storage = open(root.path());
    let store = typed(&storage, &MockClock::starting_at(1000));
    let first = create(&store, None, true, json!({}));
    let second = create(&store, None, true, json!({}));

    let hold = held(&storage);
    let accepted = store.lock().stage_stop(&first.id, "explicit");
    let shutdown = storage.shutdown();
    hold.release();
    assert!(
        accepted
            .settle(&store)
            .expect("accepted before close")
            .machine
            .stopped
    );
    shutdown.wait(DEADLINE).expect("drained shutdown");

    let refused = commit(&store, |current| current.stage_stop(&second.id, "late"));
    let error = refused.expect_err("closed admission is an explicit failure");
    assert!(
        error.starts_with("blueprint_store_persistence_failed:"),
        "{error}"
    );
    assert!(store.lock().ensure_available().is_err());
    assert!(commit(&store, Store::stage_due).is_err());
}
