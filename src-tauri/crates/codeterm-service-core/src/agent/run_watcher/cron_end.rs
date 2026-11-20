use std::thread::JoinHandle;

use crate::cron::store::{commit, CronRunStatus, CronStore, RunSession};
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
    pub(super) binding_nonce: Option<&'a str>,
}

pub(crate) struct CronEndHandle {
    terminal: std::sync::mpsc::Receiver<()>,
    thread: JoinHandle<()>,
}

impl CronEndHandle {
    pub(super) fn wait_for_terminal(self) {
        let _ = self.terminal.recv_timeout(std::time::Duration::from_secs(30));
        drop(self.thread);
    }

    #[cfg(test)]
    pub(super) fn join(self) -> std::thread::Result<()> {
        self.thread.join()
    }
}

pub(super) fn finish(svc: &ServiceContext, run: EndedCronRun<'_>) -> Option<CronEndHandle> {
    let reporter = svc.clone();
    let (settled, terminal) = std::sync::mpsc::channel();
    let thread = finish_with_signal(svc, run, move |tab_id, envelope, text| {
        reporter.deliver_cron_report(tab_id, envelope, text)
    }, Some(settled))?;
    Some(CronEndHandle { terminal, thread })
}

#[cfg(test)]
fn finish_with<D>(svc: &ServiceContext, run: EndedCronRun<'_>, deliver: D) -> Option<JoinHandle<()>>
where
    D: FnOnce(&str, MessageEnvelope, &str) -> Result<(), String> + Send + 'static,
{
    finish_with_signal(svc, run, deliver, None)
}

fn finish_with_signal<D>(svc: &ServiceContext, run: EndedCronRun<'_>, deliver: D, terminal: Option<std::sync::mpsc::Sender<()>>) -> Option<JoinHandle<()>>
where
    D: FnOnce(&str, MessageEnvelope, &str) -> Result<(), String> + Send + 'static,
{
    let cron = svc.try_ext::<crate::CronState>()?.clone();
    let mut status = if run.success {
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
    let mut summary = run.summary;
    let raw_session = run.session_id.map(str::to_string);
    let cwd = run.cwd.map(str::to_string);
    let binding_nonce = run.binding_nonce.map(str::to_string);
    Some(codeterm_util::named_thread::spawn(
        "cron-run-end",
        move || {
            let active = cron.lock().find_run_by_id(&run_id).is_some_and(|(owner, run)| {
                owner == job_id && matches!(run.status, CronRunStatus::Running | CronRunStatus::AwaitingOwnerPrompt)
            });
            if !active {
                return;
            }
            let job = { cron.lock().get(&job_id).cloned() };
            if let Some((job, policy)) = job.as_ref().and_then(|job| {
                crate::service::role_policy::resolve(job.role.as_deref())
                    .map(|policy| (job, policy))
            }) {
                let now = cron.lock().now_ms();
                match policy.settle_cron(&svc, job, &tab_id, &run_id, now) {
                    Ok(settlement) => {
                        status = if settlement.success {
                            CronRunStatus::Success
                        } else {
                            CronRunStatus::Failed
                        };
                        summary = settlement.summary;
                    }
                    Err(error) => {
                        log::debug!("[cron] settle role run {run_id}: {error}");
                        status = CronRunStatus::Failed;
                        summary = format!("Run settlement failed: {error}");
                    }
                }
            }
            let session = raw_session.as_deref().map(|session_id| RunSession {
                session_id,
                cwd: cwd.as_deref(),
            });
            let fresh_transition = commit(&cron, |store| {
                store.prepare_finish_run_with_summary(&job_id, &run_id, status.clone(), session, Some(&summary), None)
            }).map(|done| done.value.fresh).unwrap_or(false);
            if !fresh_transition {
                return;
            }
            if let Some(terminal) = terminal {
                let _ = terminal.send(());
            }
            let log_file = cron.lock().log_file(&job_id, &run_id);
            if let Err(error) = log_file.and_then(|path| CronStore::create_log(&path, &log_text).map(|_| ())) {
                log::error!("Failed to save cron log: {error}");
            }
            {
                let mut store = cron.lock();
                if store.get_tab_owner(&tab_id).is_some_and(|(job, run)| job == &job_id && run == &run_id) {
                    store.unregister_tab(&tab_id);
                }
            }
            crate::cron::notify::notify_on_consecutive_failure(&svc, &cron, &job_id);
            let _ = svc.close_tab_with_precondition(&tab_id, TabCloseReason::AgentRunEnded, |mux, tab| {
                if binding_nonce.as_deref().is_some_and(|nonce| mux.get_tab(tab).and_then(|target| target.session_binding_nonce.as_deref()) != Some(nonce)) {
                    return Err("Cron runtime binding changed".into());
                }
                Ok(())
            });
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
