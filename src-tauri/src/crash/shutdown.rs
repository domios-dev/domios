//! Graceful-shutdown coordination + clean-exit sentinel.
//!
//! A clean quit (Cmd+Q, tray Quit, `./build.sh kill` / SIGTERM, Ctrl-C, the
//! updater's `graceful_shutdown`) takes the local REST API down inside the
//! shutdown window. The liveness watchdog must not mistake that for a crash.
//!
//! `SHUTTING_DOWN` is the single source of truth every graceful-exit route
//! sets; the watchdog reads it and stops writing reports. The `.clean_exit`
//! sentinel is the belt-and-suspenders mtime filter for `check_previous_crashes`
//! that catches any racey report written right at exit.

use std::sync::atomic::{AtomicBool, Ordering};

static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

/// True once any graceful-exit route has run. Real crashes (SIGSEGV/SIGABRT/
/// native + genuine mid-run panics) happen while this is `false`, so they are
/// never suppressed.
pub fn is_shutting_down() -> bool {
    SHUTTING_DOWN.load(Ordering::SeqCst)
}

/// Mark the process as gracefully shutting down and drop the clean-exit
/// sentinel. Idempotent — safe to call from every exit route.
pub fn mark_shutting_down() {
    SHUTTING_DOWN.store(true, Ordering::SeqCst);
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

/// Spawn a dedicated thread that catches SIGTERM + SIGINT (covers
/// `./build.sh kill` and Ctrl-C), marks the process shutting down, runs the
/// caller's graceful cleanup, then exits cleanly. SIGKILL can't be caught, but
/// then the in-process watchdog dies too and writes nothing — also fine.
///
/// Windows quit/close is already covered by the Tauri exit event, so this is
/// Unix-only.
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
                mark_shutting_down();
                cleanup();
                std::process::exit(0);
            });
        })
        .expect("failed to spawn shutdown signal-handler thread");
}
