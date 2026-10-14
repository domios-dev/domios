use std::sync::Arc;

use parking_lot::Mutex;
use tauri::Manager;

use super::*;
use crate::cron::store::{commit, CronCreateParams, CronRunInfo, CronStorageOptions, CronStore};
use crate::service::test_fixture::ServiceFixture;

const RUN_TAB: &str = "cron-run-tab";
const REPORT_TAB: &str = "cron-report-tab";

type Deliveries = Arc<Mutex<Vec<(String, String)>>>;

struct OneshotRun {
    fixture: ServiceFixture,
    cron: crate::CronState,
    job_id: String,
    run_id: String,
}

fn oneshot_run(report_target: Option<&str>) -> OneshotRun {
    oneshot_run_in(report_target, |fixture| {
        CronStore::new_with_path(fixture.temp.path().join("crons.json"))
    })
}

fn oneshot_run_in(
    report_target: Option<&str>,
    store: impl FnOnce(&ServiceFixture) -> CronStore,
) -> OneshotRun {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let cron: crate::CronState = Arc::new(Mutex::new(store(&fixture)));
    fixture._app.manage(cron.clone());
    fixture
        ._app
        .manage::<crate::CronWake>(Arc::new(std::sync::Condvar::new()));
    let run_id = "oneshot-run".to_string();
    let params = CronCreateParams {
        name: "once".into(),
        workspace_id: "workspace".into(),
        team_id: None,
        detach_from_team: false,
        schedule: "in 1h".into(),
        task: "run once".into(),
        description: None,
        agent: None,
        model: None,
        reasoning_effort: None,
        cwd: None,
        role: None,
        skill: None,
        creator_tab_id: None,
        report_target_tab_id: report_target.map(str::to_string),
        enabled: Some(true),
        max_history: None,
        oneshot: Some(true),
        jitter_max_ms: Some(0),
        tz: None,
        timeout_seconds: None,
    };
    let job_id = commit(&cron, |store| store.prepare_create(params.clone()))
        .unwrap()
        .value
        .id;
    let run = CronRunInfo {
        run_id: run_id.clone(),
        started_at: 1,
        finished_at: None,
        status: CronRunStatus::Running,
        tab_id: Some(RUN_TAB.into()),
        session_id: None,
        cwd: None,
        summary: None,
        report_delivery: None,
    };
    commit(&cron, |store| {
        store.prepare_record_run(&job_id, run.clone())
    })
    .unwrap();
    OneshotRun {
        fixture,
        cron,
        job_id,
        run_id,
    }
}

fn end_run(run: &OneshotRun, success: bool, summary: &str, deliveries: &Deliveries) {
    let recorded = deliveries.clone();
    finish_with(
        &run.fixture.service,
        EndedCronRun {
            tab_id: RUN_TAB,
            job_id: &run.job_id,
            run_id: &run.run_id,
            success,
            summary: summary.to_string(),
            raw_output: "",
            session_id: None,
            cwd: None,
        },
        move |tab_id, envelope, text| {
            recorded.lock().push((
                tab_id.to_string(),
                crate::service::messenger::Messenger::from(envelope).wrap(text),
            ));
            Ok(())
        },
    )
    .expect("a managed cron store settles the run")
    .join()
    .unwrap();
}

fn assert_retired_with_report(
    run: &OneshotRun,
    deliveries: &Deliveries,
    status: CronRunStatus,
    summary: &str,
) {
    let store = run.cron.lock();
    assert!(store.get(&run.job_id).is_none(), "oneshot job is retired");
    let history = store.get_history(&run.job_id);
    assert_eq!(history.len(), 1, "the ended run stays in history");
    assert_eq!(history[0].run_id, run.run_id);
    assert_eq!(history[0].status, status);
    assert_eq!(history[0].summary.as_deref(), Some(summary));
    assert_eq!(history[0].report_delivery.as_deref(), Some("delivered"));
    let log = store
        .read_log(&run.job_id, &run.run_id)
        .expect("ended run log remains available");
    assert!(log.contains(summary));

    let deliveries = deliveries.lock();
    assert_eq!(deliveries.len(), 1, "exactly one status report");
    let (target, text) = &deliveries[0];
    assert_eq!(target, REPORT_TAB);
    let context = format!("cron:once:{}", format!("{status:?}").to_lowercase());
    assert!(
        text.starts_with(&format!("<domios from=\"system\" context=\"{context}\">")),
        "{text}"
    );
    assert!(text.contains(&run.run_id), "{text}");
    assert!(text.contains(&format!("{status:?}")), "{text}");
    assert!(text.contains(summary), "{text}");
    drop(deliveries);
    drop(store);
    let visible_history = run
        .fixture
        .service
        .cron_history(&run.cron, &run.job_id)
        .expect("deleted oneshot history remains available");
    assert_eq!(visible_history.len(), 1);
    assert_eq!(visible_history[0].status, status);
}

#[test]
fn cron_failed_oneshot_keeps_history() {
    let run = oneshot_run(Some(REPORT_TAB));
    let deliveries = Deliveries::default();
    let summary = "timeout-summary-token";

    end_run(&run, false, summary, &deliveries);

    assert_retired_with_report(&run, &deliveries, CronRunStatus::Failed, summary);
}

#[test]
fn oneshot_stop_hook_end_keeps_history_and_reports() {
    let run = oneshot_run(Some(REPORT_TAB));
    let deliveries = Deliveries::default();
    let summary = "stop-hook-summary-token";

    end_run(&run, true, summary, &deliveries);

    assert_retired_with_report(&run, &deliveries, CronRunStatus::Success, summary);
}

#[test]
fn oneshot_end_without_report_target_keeps_history() {
    let run = oneshot_run(None);
    let deliveries = Deliveries::default();

    end_run(&run, false, "no-target-summary", &deliveries);

    assert!(deliveries.lock().is_empty());
    let store = run.cron.lock();
    assert!(store.get(&run.job_id).is_none());
    let history = store.get_history(&run.job_id);
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].status, CronRunStatus::Failed);
    assert_eq!(
        history[0].report_delivery.as_deref(),
        Some("no report target")
    );
}

#[test]
fn storage_cron_consumer_run_end_commits_through_storage() {
    let opened = std::sync::OnceLock::new();
    let run = oneshot_run_in(Some(REPORT_TAB), |fixture| {
        let storage = crate::storage::Storage::open(
            fixture.temp.path().join("instance"),
            crate::storage::StorageConfig::default(),
        )
        .unwrap();
        let store = CronStore::open_storage(
            &storage,
            fixture.temp.path().join("crons"),
            crate::cron::clock::system_clock(),
            CronStorageOptions::default(),
            crate::config::schema::AppConfig::default()
                .general_agent
                .agent_provider,
        )
        .unwrap();
        let _ = opened.set(storage);
        store
    });
    let deliveries = Deliveries::default();
    let summary = "storage-summary-token";

    end_run(&run, false, summary, &deliveries);

    assert_retired_with_report(&run, &deliveries, CronRunStatus::Failed, summary);
    let fresh = CronStore::open_storage(
        opened.get().unwrap(),
        run.fixture.temp.path().join("crons"),
        crate::cron::clock::system_clock(),
        CronStorageOptions::default(),
        crate::config::schema::AppConfig::default()
            .general_agent
            .agent_provider,
    )
    .unwrap();
    let cache = run.cron.lock();
    assert!(fresh.get(&run.job_id).is_none());
    assert_eq!(
        fresh.get_history(&run.job_id),
        cache.get_history(&run.job_id)
    );
}

#[test]
fn cron_settle_async_runtime_progresses_while_cron_writer_is_held() {
    let run = oneshot_run(None);
    let writer = run.cron.lock();
    let (ready, progressed) = std::sync::mpsc::channel();
    let svc = run.fixture.service.clone();
    let job_id = run.job_id.clone();
    let run_id = run.run_id.clone();
    let caller = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                let settlement = finish_with(
                    &svc,
                    EndedCronRun {
                        tab_id: RUN_TAB,
                        job_id: &job_id,
                        run_id: &run_id,
                        success: true,
                        summary: "held-writer-summary".into(),
                        raw_output: "",
                        session_id: None,
                        cwd: None,
                    },
                    |_, _, _| Ok(()),
                )
                .expect("managed cron state starts settlement");
                tokio::task::yield_now().await;
                ready.send(settlement).unwrap();
            });
    });

    let settlement = progressed
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("the async caller progresses while the cron writer is held");
    drop(writer);
    caller.join().unwrap();
    settlement.join().unwrap();

    let cron = run.cron.lock();
    let history = cron.get_history(&run.job_id);
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].status, CronRunStatus::Success);
}
