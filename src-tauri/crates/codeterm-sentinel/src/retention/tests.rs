use super::*;
use codeterm_cron::clock::{Clock, MockClock};

#[test]
fn stopped_cleanup_uses_persisted_time_and_restart_cancels() {
    let clock = MockClock::starting_at(1_000);
    let mut retention = Retention::default();
    retention.sync(true, clock.now_ms());
    assert_eq!(retention.stopped_at, Some(1_000));
    assert_eq!(retention.deletes_at, Some(1_801_000));
    retention.sync(true, 2_000);
    assert!(!retention.overdue(true, 1_800_999));
    let restored: Retention =
        serde_json::from_str(&serde_json::to_string(&retention).unwrap()).unwrap();
    assert!(restored.overdue(true, 1_801_000));
    assert!(restored.overdue(true, 9_000_000));
    retention.sync(false, 1_500_000);
    assert_eq!(retention.stopped_at, None);
    assert_eq!(retention.deletes_at, None);
    assert!(!retention.overdue(false, 9_000_000));
    retention.sync(true, 10_000_000);
    assert_eq!(retention.deletes_at, Some(11_800_000));
}

#[test]
fn stop_paths_record_time_and_completed_watch_update_gets_a_fresh_budget() {
    use crate::{
        machine, provenance,
        store::{Owner, Store},
    };
    let root = tempfile::tempdir().unwrap();
    let clock = MockClock::starting_at(1_000);
    let mut store = Store::new(root.path().join("jobs.json"), clock.clone());
    let owner = || Owner {
        creator_tab_id: "creator".into(),
        team_id: Some("team".into()),
        workspace_id: "workspace".into(),
        cwd: "/tmp".into(),
    };
    let create = |until: Option<&str>, timeout: Option<&str>| serde_json::json!({"schema_version":2,"kind":"watch","name":if until.is_some(){"completed release watcher"}else{"expiring release watcher"},"steps":[{"id":"source","uses":"probe","with":{"command":{"lit":"status"}},"timeout":{"lit":timeout.unwrap_or("1h")}},{"id":"decide","uses":"ask","with":{"question":{"lit":"Done?"}},"buckets":["done"],"matches":{"done":{"lit":"done"}},"until":until,"routes":{"done":"notice"}},{"id":"notice","uses":"notify"}]});
    let expiring = store
        .create_document(create(None, Some("1m")), owner(), false)
        .unwrap();
    clock.set_ms(61_000);
    assert!(store.due().unwrap().is_empty());
    assert_eq!(
        store.get(&expiring.id).unwrap().cleanup.stopped_at,
        Some(61_000)
    );

    let completed = store
        .create_document(create(Some("done"), None), owner(), false)
        .unwrap();
    let lease = store.due().unwrap().remove(0);
    let mut job = lease.job.clone();
    let scope = provenance::Scope::from(&job);
    machine::evaluate(
        &mut job.machine,
        b"done",
        &job.buckets,
        &job.routes,
        job.until.as_deref(),
        scope,
        || {
            provenance::Classification::regex(machine::Verdict {
                bucket: "done".into(),
                confidence: 1.0,
            })
        },
    );
    assert!(job.machine.stopped);
    assert!(store
        .accept_transition(&job, "done", "completed-run")
        .unwrap());
    store.release(&job.id);
    let stopped = store.get(&completed.id).unwrap().clone();
    assert_eq!(stopped.cleanup.stopped_at, Some(61_000));
    let update: crate::amend::Update = serde_json::from_value(serde_json::json!({
        "name":"next release watcher", "question":"Done?", "buckets":["done"],
        "every":"20s", "matches":{"done":"done"}, "until":"done", "max_transitions":1
    }))
    .unwrap();
    let prepared = update.prepare(&stopped, "creator", false).unwrap();
    let staged = store.stage_update_definition(1, "creator", 61_000, prepared);
    let store = std::sync::Arc::new(parking_lot::Mutex::new(store));
    let updated = staged.settle(&store).unwrap();
    assert_eq!(updated.transitions.len(), 1);
    assert_eq!(updated.cleanup.transition_offset, 1);
    assert_eq!(updated.machine.last_bucket, None);
    assert_eq!(updated.cleanup.deletes_at, None);
    assert!(!crate::store::transition_budget_spent(&updated));
    let lease = store.lock().due().unwrap().remove(0);
    assert!(store
        .lock()
        .accept_transition(&lease.job, "done", "fresh-run")
        .unwrap());
    store.lock().release(&lease.job.id);
    assert!(store.lock().due().unwrap().is_empty());
    let store = store.lock();
    let limited = store.get(&lease.job.id).unwrap();
    assert_eq!(
        limited.last_reason.as_deref(),
        Some("transition_budget_exhausted")
    );
    assert_eq!(limited.cleanup.stopped_at, Some(61_000));
}
