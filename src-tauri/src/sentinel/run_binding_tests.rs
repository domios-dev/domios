use std::sync::Arc;
use std::time::Duration;

use super::run_binding::{
    active_event_run, cron_fired_event, replacement_event, PendingRunBinding,
};
use crate::cron::clock::MockClock;
use crate::cron::store::{CronJob, CronRunInfo, CronRunStatus};
use crate::sentinel::source::{RunBinding, TabBinding};
use crate::sentinel::storage::commit;
use crate::sentinel::store::{Create, Owner, Sentinel, Store};
use crate::storage::{Storage, StorageConfig};
use parking_lot::Mutex;

fn for_backends(mut run: impl FnMut(&Arc<Mutex<Store>>, &Arc<MockClock>)) {
    for typed in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let clock = MockClock::starting_at(1_000);
        let storage = typed.then(|| Storage::open(root.path(), StorageConfig::default()).unwrap());
        let store = Arc::new(Mutex::new(match storage.as_ref() {
            Some(storage) => Store::with_storage(storage, clock.clone()),
            None => Store::new(root.path().join("sentinels.json"), clock.clone()),
        }));
        run(&store, &clock);
        drop(store);
        if let Some(storage) = storage {
            storage.shutdown().wait(Duration::from_secs(30)).unwrap();
        }
    }
}

fn insert(store: &Mutex<Store>) -> Sentinel {
    let job = store
        .lock()
        .prepare(
            Create {
                name: None,
                probe: None,
                observe: Some(TabBinding::CronRun("cron-nightly".into())),
                question: "Which run state?".into(),
                buckets: vec!["waiting".into()],
                every: "10s".into(),
                routes: Default::default(),
                matches: Default::default(),
                until: None,
                timeout: None,
                paused: false,
                handlers: None,
                max_transitions: None,
                max_actions: None,
                escalation_timeout_ms: None,
            },
            Owner {
                creator_tab_id: "creator-tab".into(),
                team_id: Some("team-one".into()),
                workspace_id: "workspace-one".into(),
                cwd: "/workspace".into(),
            },
            false,
        )
        .unwrap();
    let job = commit(store, |store| store.stage_insert(job)).unwrap();
    job
}

#[test]
fn sentinel_run_bind_replacement_resets_and_fences_old_lease_both_backends() {
    for_backends(|store, clock| {
        let job = insert(store);
        assert!(commit(store, Store::stage_due).unwrap().is_empty());
        let first = commit(store, |store| {
            store.stage_bind_run(&job.id, "cron-nightly", "run-one", "tab-one")
        })
        .unwrap();
        assert!(first.changed);
        assert!(first.replaced.is_none());
        assert!(store.lock().wake_pending.contains(&job.id));
        let first_lease = commit(store, Store::stage_due).unwrap().pop().unwrap();
        assert!(!store.lock().wake_pending.contains(&job.id));
        let mut outcome = first_lease.job.clone();
        outcome.machine.last_hash = Some("episode-hash".into());
        outcome.machine.last_bucket = Some("waiting".into());
        outcome.machine.held = Some("pending-escalation".into());
        outcome.last_reason = Some("held".into());
        assert!(commit(store, |store| store.stage_finish(&outcome)).unwrap());
        store.lock().release(&job.id);
        clock.advance_ms(10_000);

        let old_lease = commit(store, Store::stage_due).unwrap().pop().unwrap();
        assert_eq!(old_lease.job.run.as_ref().unwrap().run_id, "run-one");
        let old_revision = old_lease.job.revision;
        let replacement = commit(store, |store| {
            store.stage_bind_run(&job.id, "cron-nightly", "run-two", "tab-two")
        })
        .unwrap();
        assert!(replacement.changed);
        assert_eq!(replacement.replaced.unwrap().run_id, "run-one");
        assert!(store.lock().wake_pending.contains(&job.id));
        assert!(old_lease.is_cancelled());
        assert!(!commit(store, |store| store.stage_finish(&old_lease.job)).unwrap());

        let current = store.lock().get(&job.id).unwrap().clone();
        assert_eq!(current.revision, old_revision + 1);
        assert_eq!(current.run.as_ref().unwrap().run_id, "run-two");
        assert_eq!(current.run.as_ref().unwrap().tab_id, "tab-two");
        assert!(current.machine.last_hash.is_none());
        assert!(current.machine.last_bucket.is_none());
        assert!(current.machine.held.is_none());
        store.lock().release(&job.id);
        let new_lease = commit(store, Store::stage_due).unwrap().pop().unwrap();
        assert_eq!(new_lease.job.run.as_ref().unwrap().run_id, "run-two");
        store.lock().release(&job.id);
        assert!(!store.lock().wake_tab("tab-one"));
        assert!(store.lock().wake_tab("tab-two"));
        let wake_lease = commit(store, Store::stage_due).unwrap().pop().unwrap();
        assert_eq!(wake_lease.job.run.as_ref().unwrap().run_id, "run-two");
        store.lock().release(&job.id);
    });
}

#[test]
fn sentinel_run_bind_finished_and_tab_gone_unbind_without_probing_both_backends() {
    for_backends(|store, _| {
        for (run_id, tab_id, reason) in [
            ("run-finished", "tab-finished", "cron_run_finished"),
            ("run-tab-gone", "tab-gone", "tab_gone"),
        ] {
            let job = insert(store);
            let binding = RunBinding {
                run_id: run_id.into(),
                tab_id: tab_id.into(),
                bound_at: 1_000,
            };
            assert!(
                commit(store, |store| {
                    store.stage_bind_run(&job.id, "cron-nightly", run_id, tab_id)
                })
                .unwrap()
                .changed
            );
            let lease = commit(store, Store::stage_due).unwrap().pop().unwrap();
            let mut outcome = lease.job.clone();
            outcome.machine.last_hash = Some("episode-hash".into());
            outcome.machine.last_bucket = Some("waiting".into());
            outcome.machine.held = Some("pending-escalation".into());
            assert!(commit(store, |store| store.stage_finish(&outcome)).unwrap());
            let unbound = commit(store, |store| {
                store.stage_unbind_run(&job.id, &binding, reason)
            })
            .unwrap();
            assert!(unbound.changed);
            assert!(lease.is_cancelled());
            assert!(!commit(store, |store| store.stage_finish(&lease.job)).unwrap());
            store.lock().release(&job.id);
            let current = store.lock().get(&job.id).unwrap().clone();
            assert!(current.run.is_none());
            assert_eq!(current.last_reason.as_deref(), Some(reason));
            assert!(current.machine.last_hash.is_none());
            assert!(current.machine.last_bucket.is_none());
            assert!(current.machine.held.is_none());
            assert!(commit(store, Store::stage_due).unwrap().is_empty());
        }
    });
}

#[test]
fn sentinel_run_bind_forged_event_tab_is_refused() {
    let run = run_info("run-authoritative", "tab-authoritative");
    let cron = cron_job(run.clone());
    let event = cron_fired_event(
        r#"{"cron_id":"cron-nightly","run_id":"run-authoritative","tab_id":"tab-authoritative","tab_id":"tab-authoritative"}"#,
    )
    .unwrap();
    let forged_event = cron_fired_event(
        r#"{"cron_id":"cron-nightly","run_id":"run-authoritative","tab_id":"tab-forged"}"#,
    )
    .unwrap();
    assert_eq!(event.tab_id, "tab-authoritative");
    assert!(cron_fired_event(&"x".repeat(1025)).is_none());
    let verified = active_event_run(&cron, &run, &event.tab_id).unwrap();
    assert_eq!(verified.tab_id, "tab-authoritative");
    let newer_cron = cron_job(run_info("run-newer", "tab-newer"));
    assert!(active_event_run(&newer_cron, &run, &event.tab_id).is_none());
    for_backends(|store, _| {
        let job = insert(store);
        assert!(
            commit(store, |store| {
                store.stage_bind_run(
                    &job.id,
                    &verified.cron_id,
                    &verified.run_id,
                    &verified.tab_id,
                )
            })
            .unwrap()
            .changed
        );
        assert_eq!(
            store
                .lock()
                .get(&job.id)
                .unwrap()
                .run
                .as_ref()
                .unwrap()
                .tab_id,
            "tab-authoritative"
        );

        let forged_job = insert(store);
        assert!(active_event_run(&cron, &run, &forged_event.tab_id).is_none());
        assert!(store.lock().get(&forged_job.id).unwrap().run.is_none());
    });
}

#[test]
fn sentinel_run_bind_replacement_journals_run_replaced() {
    let root = tempfile::tempdir().unwrap();
    let clock = MockClock::starting_at(1_000);
    let store = Mutex::new(Store::new(root.path().join("sentinels.json"), clock));
    let job = insert(&store);
    let pending = PendingRunBinding {
        cron_id: "cron-nightly".into(),
        run_id: "run-new".into(),
        tab_id: "tab-new".into(),
        started_at: 2_000,
        team_id: Some("team-one".into()),
        workspace_id: "workspace-one".into(),
    };
    let previous = RunBinding {
        run_id: "run-old".into(),
        tab_id: "tab-old".into(),
        bound_at: 1_000,
    };
    let runtime = crate::service::supporter::SupporterRuntime::new(root.path().into());
    runtime
        .sentinel_event(replacement_event(&job, &pending, &previous))
        .unwrap();
    let journal = std::fs::read_to_string(root.path().join("journal.jsonl")).unwrap();
    let event: serde_json::Value = serde_json::from_str(journal.lines().next().unwrap()).unwrap();
    assert_eq!(event["action"], "run_replaced");
    assert_eq!(event["old_run_id"], "run-old");
    assert_eq!(event["new_run_id"], "run-new");
}

fn cron_job(run: CronRunInfo) -> CronJob {
    CronJob {
        id: "cron-nightly".into(),
        name: "nightly".into(),
        description: None,
        workspace_id: "workspace-one".into(),
        team_id: Some("team-one".into()),
        schedule: "0 * * * *".into(),
        enabled: true,
        agent: "claude".into(),
        model: None,
        reasoning_effort: None,
        task: "run task".into(),
        cwd: None,
        role: None,
        skill: None,
        creator_tab_id: None,
        report_target_tab_id: None,
        oneshot: false,
        max_history: 10,
        created_at: 0,
        last_run: Some(run),
        next_run: None,
        jitter_max_ms: Some(0),
        tz: None,
        timeout_seconds: None,
        consecutive_failures: 0,
        protected: false,
    }
}

fn run_info(run_id: &str, tab_id: &str) -> CronRunInfo {
    CronRunInfo {
        run_id: run_id.into(),
        started_at: 1_000,
        finished_at: None,
        status: CronRunStatus::Running,
        tab_id: Some(tab_id.into()),
        session_id: None,
        cwd: None,
        summary: None,
        report_delivery: None,
    }
}
