use codeterm_service_core::crash::exit::{
    execute_exit_plan, plan_run_exit, shutdown_state_persistence_result,
};
use codeterm_service_core::multiplexer::commands::MuxState;
use codeterm_service_core::session::store::SaveOutcome;
use codeterm_service_core::KeepAwakeStateHandle;
use tauri::Manager;

fn persist_storage_desk_snapshot(
    service: &codeterm_service_core::service::ServiceContext,
    mux: &MuxState,
) -> Result<(), String> {
    match codeterm_service_core::service::session_persist::write_session_snapshot(service, mux) {
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

/// Provider-update waiter join, session snapshot, mem WAL checkpoint. Idempotent
/// and side-effect-free for process liveness, so every exit signal runs it.
pub fn persist_state(app: &codeterm_host_runtime::app_runtime::AppHandle) {
    log::info!("[shutdown] state persistence started");
    let mut errors = Vec::new();
    if let Some(service) = app.try_state::<codeterm_service_core::service::ServiceState>() {
        if let Err(error) = persist_storage_desk_snapshot(service.inner(), &service.mux) {
            errors.push(error);
        }
        if let Err(error) =
            codeterm_service_core::service::session_persist::stop_worker(service.inner())
        {
            log::warn!("[shutdown] desk persistence worker shutdown failed: {error}");
            errors.push(error);
        }
    }
    if let Some(storage) = app.try_state::<codeterm_service_core::startup::StorageAuthority>() {
        if let Err(error) = codeterm_service_core::startup::shutdown_bounded(storage.inner()) {
            log::error!("[shutdown] Storage shutdown failed: {error}");
            errors.push(error.to_string());
        }
    }

    if let Some(svc) = app.try_state::<codeterm_service_core::service::ServiceState>() {
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
pub fn declare_clean_exit(app: &codeterm_host_runtime::app_runtime::AppHandle) {
    log::info!("[shutdown] clean-exit declaration started");
    codeterm_service_core::crash::shutdown::mark_shutting_down();
    if let Some(state) = app.try_state::<KeepAwakeStateHandle>() {
        codeterm_service_core::keepawake::cleanup_active_children_for_exit_with_state(&state);
    } else {
        codeterm_service_core::keepawake::cleanup_active_children_for_exit();
    }
    codeterm_service_core::clear_webview_journal_for_exit();
    log::info!("[shutdown] clean-exit declaration completed");
}

/// Starts closing surface browsers so they flush profiles and die with the daemon, not after it.
fn begin_surface_shutdown(
    app: &codeterm_host_runtime::app_runtime::AppHandle,
) -> Option<codeterm_service_core::service::surface::SurfaceShutdown> {
    app.try_state::<codeterm_service_core::service::ServiceState>()
        .map(|svc| codeterm_service_core::service::surface::begin_surface_shutdown(svc.inner()))
}

/// Signal-driven graceful exit. Tray Quit and the macOS terminate hook reach
/// `handle_run_exit` instead, so only the unix signal handlers need this.
#[cfg(unix)]
pub fn graceful_exit_cleanup(app: &codeterm_host_runtime::app_runtime::AppHandle) {
    let sidecars = codeterm_service_core::crash::exit::begin_sidecar_shutdown();
    let surfaces = begin_surface_shutdown(app);
    declare_clean_exit(app);
    persist_state(app);
    if let Some(surfaces) = surfaces {
        surfaces.wait();
    }
    sidecars.wait();
}

/// `RunEvent::Exit` entry point: always persist, declare only on a genuine quit.
pub fn handle_run_exit(app: &codeterm_host_runtime::app_runtime::AppHandle) {
    log::info!("[shutdown] RunEvent::Exit received");
    let plan = plan_run_exit(
        cfg!(target_os = "macos"),
        codeterm_service_core::crash::shutdown::is_shutting_down(),
    );
    if !plan.declare_clean_exit {
        log::warn!(
            "[shutdown] unrequested macOS RunEvent::Exit; persisting state without clean-exit declaration"
        );
    }
    let sidecars = codeterm_service_core::crash::exit::begin_sidecar_shutdown();
    let surfaces = begin_surface_shutdown(app);
    execute_exit_plan(plan, || declare_clean_exit(app), || persist_state(app));
    if let Some(surfaces) = surfaces {
        surfaces.wait();
    }
    sidecars.wait();
    log::info!("[shutdown] RunEvent::Exit handler completed");
}
