use std::thread::JoinHandle;

use tauri::Manager;

use crate::cron::store::CronRunStatus;
use crate::multiplexer::types::TabCloseReason;
use crate::service::cron::CronRunEnd;
use crate::service::ServiceContext;

pub(super) struct EndedCronRun<'a> {
    pub(super) tab_id: &'a str,
    pub(super) job_id: &'a str,
    pub(super) run_id: &'a str,
    pub(super) success: bool,
    pub(super) summary: String,
    pub(super) raw_output: &'a str,
    pub(super) session_id: Option<&'a str>,
    pub(super) cwd: Option<&'a str>,
}

pub(super) fn finish(svc: &ServiceContext, run: EndedCronRun<'_>) {
    let reporter = svc.clone();
    let _ = finish_with(svc, run, move |tab_id, text| {
        reporter.send_text(tab_id, text)
    });
}

/// Report delivery can wait on the recipient's turn, so it runs off the watcher's caller.
fn finish_with<D>(svc: &ServiceContext, run: EndedCronRun<'_>, deliver: D) -> Option<JoinHandle<()>>
where
    D: FnOnce(&str, &str) -> Result<(), String> + Send + 'static,
{
    let cron = svc
        .app_handle
        .try_state::<crate::CronState>()?
        .inner()
        .clone();
    let status = if run.success {
        CronRunStatus::Success
    } else {
        CronRunStatus::Failed
    };
    let log_text = format!(
        "--- SUMMARY ---\n{}\n\n--- RAW OUTPUT ---\n{}",
        run.summary, run.raw_output
    );
    let fresh_transition = {
        let mut store = cron.lock();
        let _ = store.save_log(run.job_id, run.run_id, &log_text);
        if let Some(session_id) = run.session_id {
            store.set_run_session(run.job_id, run.run_id, session_id, run.cwd);
        }
        let fresh = store
            .update_run_status(run.job_id, run.run_id, status.clone())
            .unwrap_or(false);
        store.unregister_tab(run.tab_id);
        fresh
    };
    if fresh_transition {
        crate::cron::notify::notify_on_consecutive_failure(svc, &cron, run.job_id);
    }
    let _ = svc.close_tab_with_reason(run.tab_id, TabCloseReason::AgentRunEnded);

    let wake = svc
        .app_handle
        .try_state::<crate::CronWake>()
        .map(|wake| wake.inner().clone());
    let svc = svc.clone();
    let job_id = run.job_id.to_string();
    let run_id = run.run_id.to_string();
    let summary = run.summary;
    Some(std::thread::spawn(move || {
        svc.cron_settle_run_end_with(
            &cron,
            wake.as_ref(),
            CronRunEnd {
                job_id: &job_id,
                run_id: &run_id,
                status,
                summary: &summary,
            },
            deliver,
        );
        crate::api::events::emit_everywhere(&svc.app_handle, "cron-run-finished", ());
    }))
}

#[cfg(test)]
mod tests;
