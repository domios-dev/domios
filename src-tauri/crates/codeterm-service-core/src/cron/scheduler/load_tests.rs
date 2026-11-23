//! Scheduler ticks against concurrent job commits, latched wakes and restart
//! recovery: every due slot fires exactly once and no participant stalls.

use std::sync::{mpsc, Arc, Barrier};
use std::time::Duration;

use parking_lot::Mutex;

use crate::cron::clock::MockClock;
use crate::cron::store::{commit, CronCreateParams, CronStorageOptions, CronStore};
use crate::cron::wake::CronWakeSignal;
use crate::cron::{reconcile_restored_runs, RestoredCronSession};
use crate::multiplexer::state::MultiplexerState;
use crate::storage::{Storage, StorageConfig};

const FAILURE_BOUND: Duration = Duration::from_secs(60);
const CREATORS: usize = 3;
const START_MS: u64 = 1_700_000_000_000;

fn job_params(creator: usize, index: usize) -> CronCreateParams {
    serde_json::from_value(serde_json::json!({
        "name": format!("Load {creator}-{index}"),
        "workspace_id": "workspace",
        "schedule": "* * * * *",
        "task": "tick",
        "agent": "claude",
        "jitter_max_ms": 0
    }))
    .unwrap()
}

#[test]
fn cron_ticks_commits_wakes_and_recovery_progress_concurrently_without_stalls() {
    run_cron_load(2);
}

#[test]
#[ignore = "soak: run explicitly for extended scheduler contention"]
fn cron_scheduler_commit_and_recovery_soak() {
    run_cron_load(40);
}

fn run_cron_load(jobs_per_creator: usize) {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::open(temp.path().join("instance"), StorageConfig::default())
        .expect("open typed storage");
    let clock = MockClock::starting_at(START_MS);
    let cron: super::CronState = Arc::new(Mutex::new(
        CronStore::open_storage(
            &storage,
            temp.path().join("crons"),
            clock.clone(),
            CronStorageOptions::default(),
            crate::config::schema::AppConfig::default()
                .general_agent
                .agent_provider,
        )
        .expect("hydrate cron store"),
    ));
    let wake = Arc::new(CronWakeSignal::default());
    let total = CREATORS * jobs_per_creator;
    let (fired_tx, fired) = mpsc::channel();

    let scheduler = std::thread::Builder::new()
        .name("cron-load-scheduler".into())
        .spawn({
            let (cron, wake) = (cron.clone(), wake.clone());
            move || loop {
                let observed = wake.observe();
                let wait = super::tick_with(
                    &cron,
                    |spec| {
                        let fired_at = cron.lock().now_ms();
                        super::record_scheduled_fire(
                            &cron,
                            &spec.job_id,
                            &uuid::Uuid::new_v4().to_string(),
                            spec.occurrence.expect("due occurrence"),
                            Some(fired_at),
                        )
                    },
                    || {},
                );
                let recorded = {
                    let store = cron.lock();
                    store
                        .list_enabled()
                        .iter()
                        .filter(|job| !store.get_history(&job.id).is_empty())
                        .count()
                };
                if recorded == total {
                    fired_tx.send(()).unwrap();
                    return;
                }
                let timeout = match wait {
                    super::TickWait::Until(ms) => Duration::from_millis(ms),
                    _ => FAILURE_BOUND,
                };
                wake.wait_after(observed, timeout);
            }
        })
        .unwrap();

    let start = Arc::new(Barrier::new(CREATORS));
    let creators: Vec<_> = (0..CREATORS)
        .map(|creator| {
            let (cron, wake, start) = (cron.clone(), wake.clone(), start.clone());
            std::thread::Builder::new()
                .name(format!("cron-load-creator-{creator}"))
                .spawn(move || {
                    start.wait();
                    (0..jobs_per_creator)
                        .map(|index| {
                            let params = job_params(creator, index);
                            let job = commit(&cron, |store| store.prepare_create(params.clone()))
                                .expect("create cron job")
                                .value;
                            wake.notify_all();
                            job.next_run.expect("scheduled slot")
                        })
                        .max()
                        .unwrap()
                })
                .unwrap()
        })
        .collect();
    let due = creators
        .into_iter()
        .map(|creator| creator.join().unwrap())
        .max()
        .unwrap();
    clock.set_ms(due);
    wake.notify_all();
    let recovery = std::thread::Builder::new()
        .name("cron-load-recovery".into())
        .spawn({
            let cron = cron.clone();
            move || {
                let restored = RestoredCronSession::from_mux(&MultiplexerState::new().0);
                (0..total)
                    .map(|_| reconcile_restored_runs(&cron, &restored).expect("recovery commits"))
                    .map(|(_, terminalized)| terminalized)
                    .sum::<usize>()
            }
        })
        .unwrap();

    fired
        .recv_timeout(FAILURE_BOUND)
        .expect("scheduler stalled before firing every due slot");
    scheduler.join().unwrap();
    let terminalized = recovery.join().unwrap();
    assert!(terminalized <= total);
    let store = cron.lock();
    assert_eq!(store.list_enabled().len(), total);
    for job in store.list_enabled() {
        let history = store.get_history(&job.id);
        assert_eq!(history.len(), 1, "slot fired more than once for {}", job.id);
        assert_eq!(history[0].scheduled_occurrence_at, Some(due));
        assert!(job.next_run.is_some_and(|next| next > due));
    }
    drop(store);
    storage.shutdown().wait(FAILURE_BOUND).unwrap();
}
