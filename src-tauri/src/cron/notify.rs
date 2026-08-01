//! Consecutive-failure notification: real completions never surfaced a
//! repeated-failure warning before — only spawn-time failures did.

use super::scheduler::CronState;
use super::store::MAX_CONSECUTIVE_FAILURES;
use crate::service::ServiceContext;

/// First failure count that surfaces a notification — below
/// `MAX_CONSECUTIVE_FAILURES` so the user hears about trouble before the job
/// auto-disables itself.
pub const NOTIFY_AFTER_CONSECUTIVE_FAILURES: u32 = 2;

/// What (if anything) to notify about, given the job's current state. Pure —
/// no I/O — so tests can assert the decision without a live ServiceContext.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailureNotification {
    RepeatedFailure { title: String, body: String },
    AutoDisabled { title: String, body: String },
}

pub fn decide_notification(
    job_id: &str,
    name: &str,
    consecutive_failures: u32,
    enabled: bool,
    tab_label: &str,
) -> Option<FailureNotification> {
    if consecutive_failures == NOTIFY_AFTER_CONSECUTIVE_FAILURES {
        Some(FailureNotification::RepeatedFailure {
            title: format!("Cron \"{}\" failing repeatedly", name),
            body: format!(
                "{} consecutive failed runs (tab={}). `codeterm cron history {}` for detail.",
                consecutive_failures, tab_label, job_id
            ),
        })
    } else if !enabled && consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
        Some(FailureNotification::AutoDisabled {
            title: format!("Cron \"{}\" auto-disabled", name),
            body: format!(
                "{} consecutive failures — job disabled. Fix it, then `codeterm cron enable {}`.",
                consecutive_failures, job_id
            ),
        })
    } else {
        None
    }
}

/// Call only when `update_run_status` reports a fresh terminal transition —
/// a duplicate/racing completion call for the same run must not re-fire.
pub fn notify_on_consecutive_failure(
    service: &ServiceContext,
    cron_state: &CronState,
    job_id: &str,
) {
    let Some((count, name, enabled, tab_id)) = ({
        let store = cron_state.lock();
        store.get(job_id).map(|job| {
            (
                job.consecutive_failures,
                job.name.clone(),
                job.enabled,
                job.last_run.as_ref().and_then(|r| r.tab_id.clone()),
            )
        })
    }) else {
        return;
    };
    let tab_label = tab_id.unwrap_or_else(|| "-".to_string());

    match decide_notification(job_id, &name, count, enabled, &tab_label) {
        Some(FailureNotification::RepeatedFailure { title, body }) => {
            service.create_notification(title, Some(body), Some("warning"));
        }
        Some(FailureNotification::AutoDisabled { title, body }) => {
            service.create_notification(title, Some(body), Some("error"));
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cron::store::{CronCreateParams, CronRunInfo, CronRunStatus, CronStore};
    use parking_lot::Mutex;
    use std::sync::Arc;

    // ── decide_notification: pure decision, no ServiceContext needed ──

    #[test]
    fn decides_warning_at_exactly_the_notify_threshold() {
        let decision = decide_notification("job1", "flaky", 2, true, "-");
        assert!(matches!(
            decision,
            Some(FailureNotification::RepeatedFailure { .. })
        ));
    }

    #[test]
    fn decides_nothing_below_threshold_or_mid_streak() {
        assert_eq!(decide_notification("job1", "flaky", 0, true, "-"), None);
        assert_eq!(decide_notification("job1", "flaky", 1, true, "-"), None);
        assert_eq!(decide_notification("job1", "flaky", 3, true, "-"), None);
        assert_eq!(decide_notification("job1", "flaky", 4, true, "-"), None);
    }

    #[test]
    fn decides_auto_disabled_error_only_once_disabled_at_max() {
        assert_eq!(
            decide_notification("job1", "flaky", MAX_CONSECUTIVE_FAILURES, true, "-"),
            None,
            "still enabled at max count is not the auto-disable signal"
        );
        let decision = decide_notification("job1", "flaky", MAX_CONSECUTIVE_FAILURES, false, "-");
        assert!(matches!(
            decision,
            Some(FailureNotification::AutoDisabled { .. })
        ));
    }

    // ── store-level idempotency + integration with update_run_status ──

    fn store_with_job() -> (CronState, String) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crons.json");
        std::mem::forget(dir); // keep the tempdir alive for the test's lifetime
        let mut store = CronStore::new_with_path(path);
        let job = store
            .create(CronCreateParams {
                name: "flaky".into(),
                workspace_id: "ws".into(),
                team_id: None,
                schedule: "0 * * * *".into(),
                task: "do the thing".into(),
                description: None,
                agent: None,
                model: None,
                cwd: None,
                role: None,
                skill: None,
                creator_tab_id: None,
                report_target_tab_id: None,
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

    fn start_run(cron_state: &CronState, job_id: &str, run_id: &str) {
        let mut store = cron_state.lock();
        let _ = store.record_run(
            job_id,
            CronRunInfo {
                run_id: run_id.to_string(),
                started_at: 0,
                finished_at: None,
                status: CronRunStatus::Running,
                tab_id: Some("-".to_string()),
                session_id: None,
                cwd: None,
            },
        );
    }

    #[test]
    fn consecutive_failures_below_threshold_reach_notify_exactly_at_two() {
        let (cron_state, job_id) = store_with_job();
        start_run(&cron_state, &job_id, "r1");
        let fresh = cron_state
            .lock()
            .update_run_status(&job_id, "r1", CronRunStatus::Failed)
            .unwrap();
        assert!(fresh);
        assert_eq!(
            cron_state.lock().get(&job_id).unwrap().consecutive_failures,
            1
        );

        start_run(&cron_state, &job_id, "r2");
        let fresh = cron_state
            .lock()
            .update_run_status(&job_id, "r2", CronRunStatus::Failed)
            .unwrap();
        assert!(fresh);
        assert_eq!(
            cron_state.lock().get(&job_id).unwrap().consecutive_failures,
            2
        );
    }

    /// Regression: two completion paths racing on the SAME run (e.g. a pty-dead
    /// fallback and a watchdog both marking one dead tab Failed) must count once.
    #[test]
    fn duplicate_failed_update_on_same_run_counts_once() {
        let (cron_state, job_id) = store_with_job();
        start_run(&cron_state, &job_id, "r1");

        let first = cron_state
            .lock()
            .update_run_status(&job_id, "r1", CronRunStatus::Failed)
            .unwrap();
        let second = cron_state
            .lock()
            .update_run_status(&job_id, "r1", CronRunStatus::Failed)
            .unwrap();

        assert!(first, "first terminal transition must be reported as fresh");
        assert!(
            !second,
            "repeat completion of an already-terminal run must be a no-op"
        );
        assert_eq!(
            cron_state.lock().get(&job_id).unwrap().consecutive_failures,
            1,
            "duplicate Failed update must not double-count"
        );
    }

    #[test]
    fn auto_disable_still_fires_through_update_run_status_path() {
        let (cron_state, job_id) = store_with_job();
        for i in 0..MAX_CONSECUTIVE_FAILURES {
            start_run(&cron_state, &job_id, &format!("r{}", i));
            let fresh = cron_state
                .lock()
                .update_run_status(&job_id, &format!("r{}", i), CronRunStatus::Failed)
                .unwrap();
            assert!(fresh);
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
        start_run(&cron_state, &job_id, "r1");
        let _ = cron_state
            .lock()
            .update_run_status(&job_id, "r1", CronRunStatus::Failed);
        start_run(&cron_state, &job_id, "r2");
        let _ = cron_state
            .lock()
            .update_run_status(&job_id, "r2", CronRunStatus::Failed);

        start_run(&cron_state, &job_id, "r3");
        let _ = cron_state
            .lock()
            .update_run_status(&job_id, "r3", CronRunStatus::Success);

        assert_eq!(
            cron_state.lock().get(&job_id).unwrap().consecutive_failures,
            0
        );
    }
}
