//! Consecutive-failure notification — issue #179: a cron job spawned fine but
//! then failed/timed out for 4 days straight without ever surfacing anything,
//! because `update_run_status` (the path real completions go through) never
//! read `consecutive_failures`. Call this after every `update_run_status`;
//! it reads the freshly-accounted counter and fires once at the alert
//! threshold and once more if the job just auto-disabled — never on every
//! failure in between, which would just be a louder kind of silence.

use super::scheduler::CronState;
use super::store::MAX_CONSECUTIVE_FAILURES;
use crate::service::ServiceContext;

/// First failure count that surfaces a notification — below
/// `MAX_CONSECUTIVE_FAILURES` so the user hears about trouble before the job
/// auto-disables itself.
pub const NOTIFY_AFTER_CONSECUTIVE_FAILURES: u32 = 2;

pub fn notify_on_consecutive_failure(
    service: &ServiceContext,
    cron_state: &CronState,
    job_id: &str,
) {
    let Some((count, name, enabled, pane_id)) = ({
        let store = cron_state.lock();
        store.get(job_id).map(|job| {
            (
                job.consecutive_failures,
                job.name.clone(),
                job.enabled,
                job.last_run.as_ref().and_then(|r| r.pane_id.clone()),
            )
        })
    }) else {
        return;
    };

    let pane_label = pane_id.unwrap_or_else(|| "-".to_string());

    if count == NOTIFY_AFTER_CONSECUTIVE_FAILURES {
        service.create_notification(
            format!("Cron \"{}\" failing repeatedly", name),
            Some(format!(
                "{} consecutive failed runs (pane={}). `codeterm cron history {}` for detail.",
                count, pane_label, job_id
            )),
            Some("warning"),
        );
    } else if !enabled && count >= MAX_CONSECUTIVE_FAILURES {
        service.create_notification(
            format!("Cron \"{}\" auto-disabled", name),
            Some(format!(
                "{} consecutive failures — job disabled. Fix it, then `codeterm cron enable {}`.",
                count, job_id
            )),
            Some("error"),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cron::store::{CronCreateParams, CronRunInfo, CronRunStatus, CronStore};
    use parking_lot::Mutex;
    use std::sync::Arc;

    fn store_with_job() -> (CronState, String) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = CronStore::new_with_path(dir.path().join("crons.json"));
        let job = store
            .create(CronCreateParams {
                name: "flaky".into(),
                workspace_id: "ws".into(),
                schedule: "0 * * * *".into(),
                task: "do the thing".into(),
                description: None,
                agent: None,
                model: None,
                cwd: None,
                role: None,
                skill: None,
                creator_pane_id: None,
                enabled: Some(true),
                max_history: None,
                oneshot: None,
                jitter_max_ms: Some(0),
                tz: None,
                timeout_seconds: None,
            })
            .unwrap();
        (Arc::new(Mutex::new(store)), job.id)
    }

    fn fail_run(cron_state: &CronState, job_id: &str, run_id: &str) {
        let mut store = cron_state.lock();
        let _ = store.record_run(
            job_id,
            CronRunInfo {
                run_id: run_id.to_string(),
                started_at: 0,
                finished_at: None,
                status: CronRunStatus::Running,
                pane_id: Some("-".to_string()),
                tab_id: None,
                session_id: None,
                cwd: None,
            },
        );
        let _ = store.update_run_status(job_id, run_id, CronRunStatus::Failed);
    }

    #[test]
    fn consecutive_failures_below_threshold_reach_notify_exactly_at_two() {
        let (cron_state, job_id) = store_with_job();
        assert_eq!(
            cron_state.lock().get(&job_id).unwrap().consecutive_failures,
            0
        );
        fail_run(&cron_state, &job_id, "r1");
        assert_eq!(
            cron_state.lock().get(&job_id).unwrap().consecutive_failures,
            1
        );
        fail_run(&cron_state, &job_id, "r2");
        assert_eq!(
            cron_state.lock().get(&job_id).unwrap().consecutive_failures,
            2
        );
    }

    #[test]
    fn auto_disable_still_fires_through_update_run_status_path() {
        let (cron_state, job_id) = store_with_job();
        for i in 0..MAX_CONSECUTIVE_FAILURES {
            fail_run(&cron_state, &job_id, &format!("r{}", i));
        }
        let job = cron_state.lock().get(&job_id).unwrap().clone();
        assert_eq!(job.consecutive_failures, MAX_CONSECUTIVE_FAILURES);
        assert!(
            !job.enabled,
            "job must auto-disable via the completion path, not just record_run"
        );
    }

    #[test]
    fn success_resets_streak_via_update_run_status() {
        let (cron_state, job_id) = store_with_job();
        fail_run(&cron_state, &job_id, "r1");
        fail_run(&cron_state, &job_id, "r2");
        {
            let mut store = cron_state.lock();
            let _ = store.record_run(
                &job_id,
                CronRunInfo {
                    run_id: "r3".to_string(),
                    started_at: 0,
                    finished_at: None,
                    status: CronRunStatus::Running,
                    pane_id: Some("p3".to_string()),
                    tab_id: None,
                    session_id: None,
                    cwd: None,
                },
            );
            let _ = store.update_run_status(&job_id, "r3", CronRunStatus::Success);
        }
        assert_eq!(
            cron_state.lock().get(&job_id).unwrap().consecutive_failures,
            0
        );
    }
}
