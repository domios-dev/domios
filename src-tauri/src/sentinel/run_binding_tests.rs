use std::sync::Arc;
use std::time::Duration;

use super::run_binding::{
    active_event_run, cron_fired_event, replacement_event, PendingRunBinding,
};
use crate::cron::clock::MockClock;
use crate::cron::store::{CronJob, CronRunInfo, CronRunStatus};
use crate::sentinel::handlers::{Definition, Parameters};
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
    insert_with_escalation(store, false)
}

fn insert_escalating(store: &Mutex<Store>) -> Sentinel {
    insert_with_escalation(store, true)
}

fn insert_with_escalation(store: &Mutex<Store>, escalating: bool) -> Sentinel {
    insert_with_options(store, escalating, None)
}

fn insert_with_options(
    store: &Mutex<Store>,
    escalating: bool,
    max_transitions: Option<u16>,
) -> Sentinel {
    let handlers = escalating.then(|| {
        [(
            "other".to_string(),
            vec![Definition {
                id: "escalate".into(),
                kind: "escalate".into(),
                parameters: Parameters {
                    target: Some("creator".into()),
                    ..Parameters::default()
                },
            }],
        )]
        .into_iter()
        .collect()
    });
    let job = store
        .lock()
        .prepare(
            Create {
                terminal: None,
                session: false,
                name: Some("cron-run-watch".into()),
                probe: None,
                observe: Some(TabBinding::CronRun("cron-nightly".into())),
                input: None,
                question: "Which run state?".into(),
                buckets: vec!["waiting".into()],
                every: "10s".into(),
                routes: Default::default(),
                model_classification: false,
                matches: [("waiting".to_string(), ".*".to_string())]
                    .into_iter()
                    .collect(),
                rearm_on_change: Default::default(),
                until: None,
                timeout: escalating.then(|| "1h".into()),
                paused: false,
                handlers,
                max_transitions,
                max_actions: None,
                escalation_timeout_ms: escalating.then_some(5_000),
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
    commit(store, |store| store.stage_insert(job)).unwrap()
}

#[test]
fn sentinel_cron_run_transition_budget_resets_without_dropping_history_both_backends() {
    for_backends(|store, _| {
        let job = insert_with_options(store, false, Some(1));
        assert!(
            commit(store, |store| {
                store.stage_bind_run(&job.id, "cron-nightly", "run-one", "tab-one")
            })
            .unwrap()
            .changed
        );
        let first = commit(store, Store::stage_due).unwrap().pop().unwrap();
        assert!(commit(store, |store| {
            store.stage_accept_transition(&first.job, "waiting", "transition-one")
        })
        .unwrap());
        store.lock().release(&job.id);

        let first_binding = RunBinding {
            run_id: "run-one".into(),
            tab_id: "tab-one".into(),
            bound_at: 1_000,
        };
        assert!(
            commit(store, |store| {
                store.stage_unbind_run(&job.id, &first_binding, "cron_run_finished")
            })
            .unwrap()
            .changed
        );
        assert!(
            commit(store, |store| {
                store.stage_bind_run(&job.id, "cron-nightly", "run-two", "tab-two")
            })
            .unwrap()
            .changed
        );

        let second = commit(store, Store::stage_due).unwrap().pop().unwrap();
        assert_eq!(second.job.run_transitions, 0);
        assert!(commit(store, |store| {
            store.stage_accept_transition(&second.job, "waiting", "transition-two")
        })
        .unwrap());
        let current_store = store.lock();
        let current = current_store.get(&job.id).unwrap();
        assert_eq!(current.transitions.len(), 2);
        assert_eq!(current.run_transitions, 1);
        assert!(!current.machine.stopped);
        drop(current_store);
        store.lock().release(&job.id);
        assert!(commit(store, Store::stage_due).unwrap().is_empty());
        let notices = store.lock().take_stop_notices();
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].creator_tab_id, "creator-tab");
        assert_eq!(notices[0].reason, "transition_budget_exhausted");
        assert!(store.lock().take_stop_notices().is_empty());
    });
}

fn persist_held(store: &Mutex<Store>, job: &Sentinel) {
    let transition_id = format!("held-{}", job.id);
    assert!(commit(store, |store| {
        store.stage_accept_transition(job, crate::storage::OTHER_BUCKET, &transition_id)
    })
    .unwrap());
    let mut outcome = store.lock().get(&job.id).unwrap().clone();
    outcome.machine.last_hash = Some("episode-hash".into());
    outcome.machine.last_bucket = Some(crate::storage::OTHER_BUCKET.into());
    outcome.machine.held = Some(transition_id);
    outcome.last_reason = Some("held".into());
    assert!(commit(store, |store| store.stage_finish(&outcome)).unwrap());
}

#[test]
fn sentinel_run_bind_replacement_resets_and_fences_old_lease_both_backends() {
    for_backends(|store, _clock| {
        let job = insert_escalating(store);
        assert!(commit(store, Store::stage_due).unwrap().is_empty());
        let first = commit(store, |store| {
            store.stage_bind_run(&job.id, "cron-nightly", "run-one", "tab-one")
        })
        .unwrap();
        assert!(first.changed);
        assert!(first.replaced.is_none());
        assert!(store.lock().wake_pending.contains(&job.id));
        let old_lease = commit(store, Store::stage_due).unwrap().pop().unwrap();
        assert!(!store.lock().wake_pending.contains(&job.id));
        persist_held(store, &old_lease.job);
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
            let job = insert_escalating(store);
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
            persist_held(store, &lease.job);
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
