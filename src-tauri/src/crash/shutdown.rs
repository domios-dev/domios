//! Graceful-shutdown coordination + clean-exit sentinel, so the liveness watchdog
//! doesn't read a clean quit (which takes the REST API down) as a crash.
//! `SHUTDOWN_CAUSE` is the source of truth every exit route sets; the `.clean_exit`
//! sentinel is the mtime backstop catching a report written racily right at exit.

use std::sync::atomic::{AtomicU8, Ordering};

use crate::crash::lifecycle::ShutdownReason;

const NOT_SHUTTING_DOWN: u8 = 0;

static SHUTDOWN_CAUSE: AtomicU8 = AtomicU8::new(NOT_SHUTTING_DOWN);

fn encode(reason: ShutdownReason) -> u8 {
    match reason {
        ShutdownReason::UserExit => 1,
        ShutdownReason::Update => 2,
        ShutdownReason::Restart => 3,
        ShutdownReason::Signal => 4,
    }
}

fn decode(value: u8) -> Option<ShutdownReason> {
    match value {
        1 => Some(ShutdownReason::UserExit),
        2 => Some(ShutdownReason::Update),
        3 => Some(ShutdownReason::Restart),
        4 => Some(ShutdownReason::Signal),
        _ => None,
    }
}

/// True once a graceful-exit route has run; real crashes happen while it's `false`, so they're never suppressed.
pub fn is_shutting_down() -> bool {
    active_shutdown().is_some()
}

/// The cause recorded by the first graceful-exit route that fired, if any.
pub fn active_shutdown() -> Option<ShutdownReason> {
    let stored = decode(SHUTDOWN_CAUSE.load(Ordering::SeqCst));
    let resolved = resolve_shutdown(stored, os_session_ending());
    if stored.is_none() {
        if let Some(reason) = resolved {
            mark_shutting_down_for(reason);
        }
    }
    resolved
}

/// An OS session end terminates console providers before the app sees ExitRequested.
fn resolve_shutdown(
    stored: Option<ShutdownReason>,
    os_session_ending: bool,
) -> Option<ShutdownReason> {
    stored.or(os_session_ending.then_some(ShutdownReason::Signal))
}

#[cfg(windows)]
fn os_session_ending() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_SHUTTINGDOWN};
    unsafe { GetSystemMetrics(SM_SHUTTINGDOWN) != 0 }
}

#[cfg(not(windows))]
fn os_session_ending() -> bool {
    false
}

/// Mark the process as gracefully shutting down and drop the clean-exit
/// sentinel. Idempotent — safe to call from every exit route.
pub fn mark_shutting_down() {
    mark_shutting_down_for(ShutdownReason::UserExit);
}

pub fn mark_shutting_down_for(reason: ShutdownReason) {
    let first_request = SHUTDOWN_CAUSE
        .compare_exchange(
            NOT_SHUTTING_DOWN,
            encode(reason),
            Ordering::SeqCst,
            Ordering::SeqCst,
        )
        .is_ok();
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
                mark_shutting_down_for(ShutdownReason::Signal);
                cleanup();
                std::process::exit(0);
            });
        })
        .expect("failed to spawn shutdown signal-handler thread");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_graceful_route_reason_reads_back_as_an_armed_shutdown() {
        for reason in [
            ShutdownReason::UserExit,
            ShutdownReason::Update,
            ShutdownReason::Restart,
            ShutdownReason::Signal,
        ] {
            let encoded = encode(reason);
            assert_ne!(encoded, NOT_SHUTTING_DOWN);
            assert_eq!(decode(encoded), Some(reason));
        }
    }

    #[test]
    fn os_session_end_reads_as_a_shutdown_before_any_exit_route_runs() {
        assert_eq!(resolve_shutdown(None, false), None);
        assert_eq!(resolve_shutdown(None, true), Some(ShutdownReason::Signal));
        assert_eq!(
            resolve_shutdown(Some(ShutdownReason::Update), true),
            Some(ShutdownReason::Update)
        );
    }
}
