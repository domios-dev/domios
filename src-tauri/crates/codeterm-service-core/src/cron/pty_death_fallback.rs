//! Fails a cron run whose agent PTY died before the run completed.
//!
//! The cron lock is only taken briefly for the owner lookup and the unregister; terminal
//! capture, the log write and the run commit all happen with no cron lock held.

use std::thread::JoinHandle;

use crate::cron::store::{finish_run, CronRunStatus, CronStore};
use crate::service::ServiceContext;
use crate::CronState;

pub(crate) fn fail_run_on_pty_death(
    svc: &ServiceContext,
    cron: &CronState,
    tab_id: &str,
    pty_id: &str,
) -> Option<JoinHandle<()>> {
    let svc = svc.clone();
    let cron = cron.clone();
    let tab_id = tab_id.to_string();
    let pty_id = pty_id.to_string();
    Some(std::thread::spawn(move || {
        let settled = settle(
            &cron,
            &tab_id,
            || svc.output_buf.lock().get_text(&pty_id).unwrap_or_default(),
            |job_id| crate::cron::notify::notify_on_consecutive_failure(&svc, &cron, job_id),
        );
        if let Some((job_id, run_id)) = settled {
            log::info!("[pty-death] cron Tab exited: job={job_id}, run={run_id}");
            crate::events::emit_everywhere(&svc.app_handle, "cron-run-finished", ());
        }
    }))
}

/// An existing log means a completion path already owns the run, so the run is left as is.
fn settle(
    cron: &CronState,
    tab_id: &str,
    capture: impl FnOnce() -> String,
    notify: impl FnOnce(&str),
) -> Option<(String, String)> {
    let (job_id, run_id, log_file) = {
        let store = cron.lock();
        let (job_id, run_id) = store.get_tab_owner(tab_id).cloned()?;
        let log_file = store.log_file(&job_id, &run_id);
        (job_id, run_id, log_file)
    };
    let text = crate::pty::ansi::clean_terminal_output(&capture());
    let fail = || finish_run(cron, &job_id, &run_id, CronRunStatus::Failed, None).unwrap_or(false);
    let fresh = match log_file.and_then(|path| CronStore::create_log(&path, &text)) {
        Ok(true) => fail(),
        Ok(false) => false,
        Err(error) => {
            log::error!("Failed to save cron log: {error}");
            fail()
        }
    };
    cron.lock().unregister_tab(tab_id);
    if fresh {
        notify(&job_id);
    }
    Some((job_id, run_id))
}

#[cfg(test)]
mod tests;
