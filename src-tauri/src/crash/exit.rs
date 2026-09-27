//! Exit cleanup, split into two independently-runnable halves.
//!
//! Persisting state is safe on any `RunEvent::Exit`, including one we never
//! asked for; declaring a clean exit (sentinel + keepawake/webview teardown) is
//! only correct on a genuine quit.

use tauri::Manager;

use crate::multiplexer::commands::MuxState;
use crate::session::store::SaveOutcome;
use crate::session::SessionState;
use crate::KeepAwakeStateHandle;
use crate::OutputBufferState;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExitPlan {
    pub(crate) declare_clean_exit: bool,
    pub(crate) persist_state: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum ShutdownSidecarLogLevel {
    Info,
    Warn,
}

#[derive(Debug, PartialEq, Eq)]
struct ShutdownSidecarLogRecord {
    level: ShutdownSidecarLogLevel,
    message: String,
}

/// macOS delivers `RunEvent::Exit` with no preceding quit request on routes we
/// never asked for; persist there anyway, but never tear down keepawake children.
pub(crate) fn plan_run_exit(is_macos: bool, shutting_down: bool) -> ExitPlan {
    ExitPlan {
        declare_clean_exit: !is_macos || shutting_down,
        persist_state: true,
    }
}

pub(crate) fn execute_exit_plan<D, P>(plan: ExitPlan, declare: D, persist: P)
where
    D: FnOnce(),
    P: FnOnce(),
{
    if plan.declare_clean_exit {
        declare();
    }
    if plan.persist_state {
        persist();
    }
}

/// The lock pair at the heart of rel#65: snapshot + save the session, then run
/// the post-save sidecar sync, without ever holding the session guard across
/// the sync's own re-entrant lock. Takes exactly the state it needs — no
/// `AppHandle`, no Tauri runtime — so the ordering is provable on its own,
/// independent of the shutdown path that happens to call it.
pub(crate) fn save_and_sync_sidecars(
    mux: &MuxState,
    session: &SessionState,
    output_buf: Option<&OutputBufferState>,
) {
    let snapshot = mux.write().to_session_snapshot();
    // Drop the session guard before sidecar sync re-enters the store.
    let save_result = { session.lock().save(&snapshot) };
    match save_result {
        Ok(outcome) => {
            match &outcome {
                SaveOutcome::Saved => log::info!("[shutdown] session snapshot saved"),
                SaveOutcome::RefusedReduction(_) => {
                    log::warn!("[shutdown] session snapshot save refused")
                }
            }
            let report = match output_buf {
                Some(output_buf) => {
                    crate::session::scrollback_sync::sync_scrollback_sidecars_after_save_with(
                        &outcome, mux, output_buf, session,
                    )
                }
                None => crate::session::scrollback_sync::skipped_report(
                    mux,
                    match &outcome {
                        SaveOutcome::Saved => "output_buffer_unavailable",
                        SaveOutcome::RefusedReduction(_) => "session_snapshot_not_saved",
                    },
                ),
            };
            log_shutdown_sidecar_report(&report);
        }
        Err(error) => {
            log::error!("[shutdown] session save failed: {error}");
            let report =
                crate::session::scrollback_sync::skipped_report(mux, "session_save_failed");
            log_shutdown_sidecar_report(&report);
        }
    }
}

fn shutdown_sidecar_log_records(
    report: &crate::session::scrollback_sync::SidecarSyncReport,
) -> Vec<ShutdownSidecarLogRecord> {
    let mut records = Vec::with_capacity(report.written.len() + report.skipped.len() + 1);
    records.extend(report.written.iter().map(|(tab_id, bytes)| {
        ShutdownSidecarLogRecord {
            level: ShutdownSidecarLogLevel::Info,
            message: format!(
                "[shutdown] scrollback sidecar tab={tab_id} result=written bytes={bytes}"
            ),
        }
    }));
    records.extend(report.skipped.iter().map(|(tab_id, reason)| {
        ShutdownSidecarLogRecord {
            level: ShutdownSidecarLogLevel::Warn,
            message: format!(
                "[shutdown] scrollback sidecar tab={tab_id} result=skipped reason={reason}"
            ),
        }
    }));
    let level = if report.skipped.is_empty() {
        ShutdownSidecarLogLevel::Info
    } else {
        ShutdownSidecarLogLevel::Warn
    };
    let summary = if report.skipped.is_empty() {
        format!(
            "[shutdown] scrollback sidecars synchronized written={} skipped=0",
            report.written.len()
        )
    } else {
        format!(
            "[shutdown] scrollback sidecar synchronization incomplete written={} skipped={}",
            report.written.len(),
            report.skipped.len()
        )
    };
    records.push(ShutdownSidecarLogRecord {
        level,
        message: summary,
    });
    records
}

fn log_shutdown_sidecar_report(report: &crate::session::scrollback_sync::SidecarSyncReport) {
    for record in shutdown_sidecar_log_records(report) {
        match record.level {
            ShutdownSidecarLogLevel::Info => log::info!("{}", record.message),
            ShutdownSidecarLogLevel::Warn => log::warn!("{}", record.message),
        }
    }
}

/// Provider-update waiter join, session snapshot, mem WAL checkpoint. Idempotent
/// and side-effect-free for process liveness, so every exit signal runs it.
pub fn persist_state(app: &tauri::AppHandle) {
    log::info!("[shutdown] state persistence started");
    if let (Some(mux), Some(session)) =
        (app.try_state::<MuxState>(), app.try_state::<SessionState>())
    {
        let output_buf = app.try_state::<OutputBufferState>();
        save_and_sync_sidecars(
            mux.inner(),
            session.inner(),
            output_buf.as_ref().map(|s| s.inner()),
        );
    }

    if let Some(svc) = app.try_state::<crate::service::ServiceState>() {
        match svc.mem.checkpoint_and_close() {
            Ok((busy, log_frames, checkpointed)) => log::info!(
                "[shutdown] mem WAL checkpointed busy={} log_frames={} checkpointed={}",
                busy,
                log_frames,
                checkpointed
            ),
            Err(e) => log::warn!("[shutdown] mem WAL checkpoint failed: {}", e),
        }
    }
    log::info!("[shutdown] state persistence completed");
}

/// Clean-exit declaration: `.clean_exit` sentinel + keepawake child teardown +
/// webview reap journal clear. Only a genuine quit may run this.
pub fn declare_clean_exit(app: &tauri::AppHandle) {
    log::info!("[shutdown] clean-exit declaration started");
    crate::crash::shutdown::mark_shutting_down();
    if let Some(state) = app.try_state::<KeepAwakeStateHandle>() {
        crate::keepawake::cleanup_active_children_for_exit_with_state(&state);
    } else {
        crate::keepawake::cleanup_active_children_for_exit();
    }
    crate::webview2_reap::clear_journal_for_exit();
    log::info!("[shutdown] clean-exit declaration completed");
}

/// Signal-driven graceful exit. Tray Quit and the macOS terminate hook reach
/// `handle_run_exit` instead, so only the unix signal handlers need this.
#[cfg(unix)]
pub fn graceful_exit_cleanup(app: &tauri::AppHandle) {
    declare_clean_exit(app);
    persist_state(app);
}

/// `RunEvent::Exit` entry point: always persist, declare only on a genuine quit.
pub fn handle_run_exit(app: &tauri::AppHandle) {
    log::info!("[shutdown] RunEvent::Exit received");
    let plan = plan_run_exit(
        cfg!(target_os = "macos"),
        crate::crash::shutdown::is_shutting_down(),
    );
    if !plan.declare_clean_exit {
        log::warn!(
            "[shutdown] unrequested macOS RunEvent::Exit; persisting state without clean-exit declaration"
        );
    }
    execute_exit_plan(plan, || declare_clean_exit(app), || persist_state(app));
    log::info!("[shutdown] RunEvent::Exit handler completed");
}
