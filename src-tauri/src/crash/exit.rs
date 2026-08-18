//! Exit cleanup, split into two independently-runnable halves.
//!
//! Persisting state is safe on any `RunEvent::Exit`, including one we never
//! asked for; declaring a clean exit (sentinel + keepawake/webview teardown) is
//! only correct on a genuine quit.

use tauri::Manager;

use crate::multiplexer::commands::MuxState;
use crate::session::SessionState;
use crate::KeepAwakeStateHandle;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExitPlan {
    pub(crate) declare_clean_exit: bool,
    pub(crate) persist_state: bool,
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

/// Session snapshot + mem WAL checkpoint. Idempotent and side-effect-free for
/// process liveness, so it runs on every exit signal we observe.
pub fn persist_state(app: &tauri::AppHandle) {
    if let (Some(mux), Some(session)) =
        (app.try_state::<MuxState>(), app.try_state::<SessionState>())
    {
        let snapshot = mux.write().to_session_snapshot();
        match session.lock().save(&snapshot) {
            Ok(outcome) => {
                log::info!("[shutdown] session snapshot saved");
                crate::session::scrollback_sync::sync_scrollback_sidecars_after_save(app, &outcome);
            }
            Err(e) => log::error!("[shutdown] session save failed: {}", e),
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
            Err(e) => log::warn!("[shutdown] mem WAL checkpoint failed: {}", e),
        }
    }
}

/// Clean-exit declaration: `.clean_exit` sentinel + keepawake child teardown +
/// webview reap journal clear. Only a genuine quit may run this.
pub fn declare_clean_exit(app: &tauri::AppHandle) {
    crate::crash::shutdown::mark_shutting_down();
    if let Some(state) = app.try_state::<KeepAwakeStateHandle>() {
        crate::keepawake::cleanup_active_children_for_exit_with_state(&state);
    } else {
        crate::keepawake::cleanup_active_children_for_exit();
    }
    crate::webview2_reap::clear_journal_for_exit();
}

/// Full graceful exit for routes that already know the quit is genuine
/// (tray Quit, caught SIGTERM/SIGINT, updater teardown).
pub fn graceful_exit_cleanup(app: &tauri::AppHandle) {
    declare_clean_exit(app);
    persist_state(app);
}

/// `RunEvent::Exit` entry point: always persist, declare only on a genuine quit.
pub fn handle_run_exit(app: &tauri::AppHandle) {
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
}
