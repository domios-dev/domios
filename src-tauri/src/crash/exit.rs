//! Exit cleanup, split into two independently-runnable halves.
//!
//! Persisting state is safe on any `RunEvent::Exit`, including one we never
//! asked for; declaring a clean exit (sentinel + keepawake/webview teardown) is
//! only correct on a genuine quit.

use tauri::Manager;

use crate::multiplexer::commands::MuxState;
use crate::session::store::SaveOutcome;
use crate::KeepAwakeStateHandle;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExitPlan {
    pub(crate) declare_clean_exit: bool,
    pub(crate) persist_state: bool,
}

#[derive(Debug, PartialEq, Eq)]
#[cfg(test)]
enum ShutdownSidecarLogLevel {
    Info,
    Warn,
}

#[derive(Debug, PartialEq, Eq)]
#[cfg(test)]
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

fn persist_storage_desk_snapshot(app: &tauri::AppHandle, mux: &MuxState) -> Result<(), String> {
    match crate::service::session_persist::write_session_snapshot(app, mux) {
        Ok(SaveOutcome::Saved) => {
            log::info!("[shutdown] Storage desk snapshot committed");
            Ok(())
        }
        Ok(SaveOutcome::RefusedReduction(refusal)) => {
            let error = format!("Storage desk snapshot refused: {refusal}");
            log::error!("[shutdown] {error}");
            Err(error)
        }
        Err(error) => {
            let failure = format!("Storage desk snapshot failed: {error}");
            log::error!("[shutdown] {failure}");
            Err(failure)
        }
    }
}

fn shutdown_state_persistence_result(errors: &[String]) -> Result<(), String> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(test)]
fn shutdown_sidecar_log_records(
    report: &crate::session::scrollback_sync::SidecarSyncReport,
) -> Vec<ShutdownSidecarLogRecord> {
    let mut records = Vec::with_capacity(report.written.len() + report.skipped.len() + 1);
    records.extend(
        report
            .written
            .iter()
            .map(|(tab_id, bytes)| ShutdownSidecarLogRecord {
                level: ShutdownSidecarLogLevel::Info,
                message: format!(
                    "[shutdown] scrollback sidecar tab={tab_id} result=written bytes={bytes}"
                ),
            }),
    );
    records.extend(
        report
            .skipped
            .iter()
            .map(|(tab_id, reason)| ShutdownSidecarLogRecord {
                level: ShutdownSidecarLogLevel::Warn,
                message: format!(
                    "[shutdown] scrollback sidecar tab={tab_id} result=skipped reason={reason}"
                ),
            }),
    );
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

/// Provider-update waiter join, session snapshot, mem WAL checkpoint. Idempotent
/// and side-effect-free for process liveness, so every exit signal runs it.
pub fn persist_state(app: &tauri::AppHandle) {
    log::info!("[shutdown] state persistence started");
    let mut errors = Vec::new();
    if let Some(mux) = app.try_state::<MuxState>() {
        if let Err(error) = persist_storage_desk_snapshot(app, mux.inner()) {
            errors.push(error);
        }
    }
    if let Err(error) = crate::service::session_persist::stop_worker(app) {
        log::warn!("[shutdown] desk persistence worker shutdown failed: {error}");
        errors.push(error);
    }
    if let Some(storage) = app.try_state::<crate::startup::StorageAuthority>() {
        if let Err(error) = crate::startup::shutdown_bounded(storage.inner()) {
            log::error!("[shutdown] Storage shutdown failed: {error}");
            errors.push(error.to_string());
        }
    }

    if let Some(svc) = app.try_state::<crate::service::ServiceState>() {
        match svc.mem.checkpoint_and_close() {
            Ok((busy, log_frames, checkpointed)) => log::info!(
                "[shutdown] mem WAL checkpointed busy={} log_frames={} checkpointed={}",
                busy,
                log_frames,
                checkpointed
            ),
            Err(e) => {
                log::warn!("[shutdown] mem WAL checkpoint failed: {}", e);
                errors.push(e.to_string());
            }
        }
    }
    match shutdown_state_persistence_result(&errors) {
        Ok(()) => log::info!("[shutdown] state persistence completed"),
        Err(_) => log::error!("[shutdown] state persistence incomplete"),
    }
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

/// Starts closing surface browsers so they flush profiles and die with the daemon, not after it.
fn begin_surface_shutdown(
    app: &tauri::AppHandle,
) -> Option<crate::service::surface::SurfaceShutdown> {
    app.try_state::<crate::service::ServiceState>()
        .map(|svc| crate::service::surface::begin_surface_shutdown(svc.inner()))
}

/// Signal-driven graceful exit. Tray Quit and the macOS terminate hook reach
/// `handle_run_exit` instead, so only the unix signal handlers need this.
#[cfg(unix)]
pub fn graceful_exit_cleanup(app: &tauri::AppHandle) {
    let sidecars = crate::service::codex_rpc::exit::begin();
    let surfaces = begin_surface_shutdown(app);
    declare_clean_exit(app);
    persist_state(app);
    if let Some(surfaces) = surfaces {
        surfaces.wait();
    }
    sidecars.wait();
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
    let sidecars = crate::service::codex_rpc::exit::begin();
    let surfaces = begin_surface_shutdown(app);
    execute_exit_plan(plan, || declare_clean_exit(app), || persist_state(app));
    if let Some(surfaces) = surfaces {
        surfaces.wait();
    }
    sidecars.wait();
    log::info!("[shutdown] RunEvent::Exit handler completed");
}
