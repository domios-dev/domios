use std::sync::Arc;

use parking_lot::Mutex;
use tauri::Manager;

use super::*;
use crate::cron::store::{CronCreateParams, CronRunInfo, CronStore};
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
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let cron: crate::CronState = Arc::new(Mutex::new(CronStore::new_with_path(
        fixture.temp.path().join("crons.json"),
    )));
    fixture._app.manage(cron.clone());
    fixture
        ._app
        .manage::<crate::CronWake>(Arc::new(std::sync::Condvar::new()));
    let run_id = "oneshot-run".to_string();
    let job_id = {
        let mut store = cron.lock();
        let job = store
            .create(CronCreateParams {
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
            })
            .unwrap();
        store
            .record_run(
                &job.id,
                CronRunInfo {
                    run_id: run_id.clone(),
                    started_at: 1,
                    finished_at: None,
                    status: CronRunStatus::Running,
                    tab_id: Some(RUN_TAB.into()),
                    session_id: None,
                    cwd: None,
                    summary: None,
                    report_delivery: None,
                },
            )
            .unwrap();
        job.id
    };
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
        move |tab_id, text| {
            recorded.lock().push((tab_id.to_string(), text.to_string()));
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

    let deliveries = deliveries.lock();
    assert_eq!(deliveries.len(), 1, "exactly one status report");
    let (target, text) = &deliveries[0];
    assert_eq!(target, REPORT_TAB);
    assert!(text.contains(&run.run_id), "{text}");
    assert!(text.contains(&format!("{status:?}")), "{text}");
    assert!(text.contains(summary), "{text}");
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn oneshot_timeout_end_keeps_history_and_reports() {
    let run = oneshot_run(Some(REPORT_TAB));
    let deliveries = Deliveries::default();
    let summary = "timeout-summary-token";

    end_run(&run, false, summary, &deliveries);

    assert_retired_with_report(&run, &deliveries, CronRunStatus::Failed, summary);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn oneshot_stop_hook_end_keeps_history_and_reports() {
    let run = oneshot_run(Some(REPORT_TAB));
    let deliveries = Deliveries::default();
    let summary = "stop-hook-summary-token";

    end_run(&run, true, summary, &deliveries);

    assert_retired_with_report(&run, &deliveries, CronRunStatus::Success, summary);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
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
