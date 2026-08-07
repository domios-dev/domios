//! Graceful-shutdown coordination + clean-exit sentinel, so the liveness watchdog
//! doesn't read a clean quit (which takes the REST API down) as a crash.
//! `SHUTTING_DOWN` is the source of truth every exit route sets; the `.clean_exit`
//! sentinel is the mtime backstop catching a report written racily right at exit.

use std::sync::atomic::{AtomicBool, Ordering};

static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);
static UPDATE_SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// True once a graceful-exit route has run; real crashes happen while it's `false`, so they're never suppressed.
pub fn is_shutting_down() -> bool {
    SHUTTING_DOWN.load(Ordering::SeqCst)
}

pub fn is_update_shutdown() -> bool {
    UPDATE_SHUTDOWN.load(Ordering::SeqCst)
}

pub fn mark_update_teardown_started() {
    UPDATE_SHUTDOWN.store(true, Ordering::SeqCst);
}

/// Mark the process as gracefully shutting down and drop the clean-exit
/// sentinel. Idempotent — safe to call from every exit route.
pub fn mark_shutting_down() {
    mark_shutting_down_for(crate::crash::lifecycle::ShutdownReason::UserExit);
}

pub fn mark_shutting_down_for(reason: crate::crash::lifecycle::ShutdownReason) {
    let first_request = !SHUTTING_DOWN.swap(true, Ordering::SeqCst);
    if first_request {
        crate::crash::record_clean_shutdown(reason);
    }
    write_clean_exit_sentinel();
}

fn clean_exit_path() -> std::path::PathBuf {
    crate::crash::report::crashes_dir().join(".clean_exit")
}

fn write_clean_exit_sentinel() {
    let path = clean_exit_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let ts = chrono::Utc::now().timestamp();
    let _ = std::fs::write(&path, ts.to_string());
}

/// Unix-seconds timestamp of the last graceful exit, if a sentinel exists.
/// `check_previous_crashes` ignores any report with mtime ≤ this.
pub(crate) fn clean_exit_timestamp() -> Option<i64> {
    std::fs::read_to_string(clean_exit_path())
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
}

/// Catch SIGTERM/SIGINT on a dedicated thread: mark shutting down, run cleanup, exit
/// cleanly. SIGKILL can't be caught, but then the watchdog dies too and writes nothing. Unix-only.
#[cfg(unix)]
pub fn install_unix_signal_handlers<F>(cleanup: F)
where
    F: FnOnce() + Send + 'static,
{
    std::thread::Builder::new()
        .name("shutdown-signal-handler".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    log::error!("[shutdown] signal-handler runtime failed: {}", e);
                    return;
                }
            };
            rt.block_on(async move {
                use tokio::signal::unix::{signal, SignalKind};
                let mut term = match signal(SignalKind::terminate()) {
                    Ok(s) => s,
                    Err(e) => {
                        log::error!("[shutdown] SIGTERM handler install failed: {}", e);
                        return;
                    }
                };
                let mut int = match signal(SignalKind::interrupt()) {
                    Ok(s) => s,
                    Err(e) => {
                        log::error!("[shutdown] SIGINT handler install failed: {}", e);
                        return;
                    }
                };
                tokio::select! {
                    _ = term.recv() => log::info!("[shutdown] SIGTERM received; graceful exit"),
                    _ = int.recv() => log::info!("[shutdown] SIGINT received; graceful exit"),
                }
                mark_shutting_down_for(crate::crash::lifecycle::ShutdownReason::Signal);
                cleanup();
                std::process::exit(0);
            });
        })
        .expect("failed to spawn shutdown signal-handler thread");
}
