use std::thread::JoinHandle;

use crate::cron::store::{finish_run, CronRunStatus, CronStore, RunSession};
use crate::multiplexer::types::TabCloseReason;
use crate::service::cron::CronRunEnd;
use crate::service::messenger::MessageEnvelope;
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
    let _ = finish_with(svc, run, move |tab_id, envelope, text| {
        reporter.deliver_cron_report(tab_id, envelope, text)
    });
}

/// Report delivery can wait on the recipient's turn, so it runs off the watcher's caller.
fn finish_with<D>(svc: &ServiceContext, run: EndedCronRun<'_>, deliver: D) -> Option<JoinHandle<()>>
where
    D: FnOnce(&str, MessageEnvelope, &str) -> Result<(), String> + Send + 'static,
{
    let cron = svc.try_ext::<crate::CronState>()?.clone();
    let status = if run.success {
        CronRunStatus::Success
    } else {
        CronRunStatus::Failed
    };
    let log_text = format!(
        "--- SUMMARY ---\n{}\n\n--- RAW OUTPUT ---\n{}",
        run.summary, run.raw_output
    );
    let wake = svc.try_ext::<crate::CronWake>().cloned();
    let svc = svc.clone();
    let tab_id = run.tab_id.to_string();
    let job_id = run.job_id.to_string();
    let run_id = run.run_id.to_string();
    let summary = run.summary;
    let raw_session = run.session_id.map(str::to_string);
    let cwd = run.cwd.map(str::to_string);
    Some(codeterm_util::named_thread::spawn(
        "cron-run-end",
        move || {
            let log_file = cron.lock().log_file(&job_id, &run_id);
            if let Err(error) =
                log_file.and_then(|path| CronStore::create_log(&path, &log_text).map(|_| ()))
            {
                log::error!("Failed to save cron log: {error}");
            }
            let session = raw_session.as_deref().map(|session_id| RunSession {
                session_id,
                cwd: cwd.as_deref(),
            });
            let fresh_transition =
                finish_run(&cron, &job_id, &run_id, status.clone(), session).unwrap_or(false);
            cron.lock().unregister_tab(&tab_id);
            if fresh_transition {
                crate::cron::notify::notify_on_consecutive_failure(&svc, &cron, &job_id);
            }
            let _ = svc.close_tab_with_reason(&tab_id, TabCloseReason::AgentRunEnded);
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
            svc.ui.emit(
                "cron-run-finished",
                serde_json::to_value(()).unwrap_or_default(),
            );
        },
    ))
}

#[cfg(test)]
mod tests;
