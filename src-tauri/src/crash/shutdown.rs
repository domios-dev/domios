//! Graceful-shutdown coordination + clean-exit sentinel, so the liveness watchdog
//! doesn't read a clean quit (which takes the REST API down) as a crash.
//! `SHUTDOWN_CAUSE` is the source of truth every exit route sets; the `.clean_exit`
//! sentinel is the mtime backstop catching a report written racily right at exit.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, LazyLock, Weak};

use parking_lot::Mutex;

use crate::crash::lifecycle::ShutdownReason;

const NOT_SHUTTING_DOWN: u8 = 0;

static SHUTDOWN_CAUSE: AtomicU8 = AtomicU8::new(NOT_SHUTTING_DOWN);
static SENTINEL_SHUTDOWN: LazyLock<ShutdownLatch> = LazyLock::new(ShutdownLatch::default);

type ShutdownCallback = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct SubscriptionState {
    reason: Option<ShutdownReason>,
    next_id: u64,
    callback: Option<(u64, ShutdownCallback)>,
}

#[derive(Clone, Default)]
pub(crate) struct ShutdownLatch {
    state: Arc<Mutex<SubscriptionState>>,
}

pub(crate) struct ShutdownSubscription {
    state: Weak<Mutex<SubscriptionState>>,
    id: u64,
}

impl ShutdownLatch {
    pub(crate) fn subscribe(
        &self,
        callback: impl FnOnce() + Send + 'static,
    ) -> Result<ShutdownSubscription, &'static str> {
        let mut callback: Option<ShutdownCallback> = Some(Box::new(callback));
        let (id, armed) = {
            let mut state = self.state.lock();
            if state.callback.is_some() {
                return Err("Sentinel shutdown subscription already occupied");
            }
            state.next_id = state
                .next_id
                .checked_add(1)
                .ok_or("Sentinel shutdown subscription identity exhausted")?;
            let id = state.next_id;
            let armed = state.reason.is_some();
            if !armed {
                state.callback = callback.take().map(|callback| (id, callback));
            }
            (id, armed)
        };
        if armed {
            if let Some(callback) = callback {
                callback();
            }
        }
        Ok(ShutdownSubscription {
            state: Arc::downgrade(&self.state),
            id,
        })
    }

    pub(crate) fn arm(&self, reason: ShutdownReason) {
        let callback = {
            let mut state = self.state.lock();
            state.reason.get_or_insert(reason);
            state.callback.take().map(|(_, callback)| callback)
        };
        if let Some(callback) = callback {
            callback();
        }
    }
}

impl Drop for ShutdownSubscription {
    fn drop(&mut self) {
        if let Some(state) = self.state.upgrade() {
            let mut state = state.lock();
            if state
                .callback
                .as_ref()
                .is_some_and(|(id, _)| *id == self.id)
            {
                state.callback = None;
            }
        }
    }
}

pub(crate) fn subscribe_sentinel_shutdown(
    callback: impl FnOnce() + Send + 'static,
) -> Result<ShutdownSubscription, &'static str> {
    SENTINEL_SHUTDOWN.subscribe(callback)
}

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
    if let Some(recorded) = decode(SHUTDOWN_CAUSE.load(Ordering::SeqCst)) {
        SENTINEL_SHUTDOWN.arm(recorded);
    }
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

#[cfg(test)]
mod subscription_tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Barrier;

    #[test]
    fn sentinel_command_shutdown_registration_before_after_and_repeated_reasons() {
        for before in [true, false] {
            let latch = ShutdownLatch::default();
            let calls = Arc::new(AtomicUsize::new(0));
            if !before {
                latch.arm(ShutdownReason::Update);
            }
            let observed = calls.clone();
            let checking = latch.clone();
            let _subscription = latch
                .subscribe(move || {
                    assert!(
                        checking.state.try_lock().is_some(),
                        "callback cannot run under registry lock"
                    );
                    observed.fetch_add(1, Ordering::SeqCst);
                })
                .unwrap();
            latch.arm(ShutdownReason::Update);
            latch.arm(ShutdownReason::Signal);
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(latch.state.lock().reason, Some(ShutdownReason::Update));
        }
    }

    #[test]
    fn sentinel_command_shutdown_concurrent_registration_and_arm_deliver_once() {
        for _ in 0..32 {
            let latch = ShutdownLatch::default();
            let barrier = Arc::new(Barrier::new(2));
            let calls = Arc::new(AtomicUsize::new(0));
            let worker_latch = latch.clone();
            let worker_barrier = barrier.clone();
            let worker = std::thread::spawn(move || {
                worker_barrier.wait();
                worker_latch.arm(ShutdownReason::Restart);
            });
            barrier.wait();
            let observed = calls.clone();
            let _subscription = latch
                .subscribe(move || {
                    observed.fetch_add(1, Ordering::SeqCst);
                })
                .unwrap();
            worker.join().unwrap();
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(latch.state.lock().reason, Some(ShutdownReason::Restart));
        }
    }

    #[test]
    fn sentinel_command_shutdown_unregistration_and_dropped_weak_owner_are_bounded() {
        let latch = ShutdownLatch::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let subscription = latch
            .subscribe(move || {
                observed.fetch_add(1, Ordering::SeqCst);
            })
            .unwrap();
        assert!(latch.subscribe(|| {}).is_err());
        drop(subscription);
        let weak = Arc::downgrade(&calls);
        let _subscription = latch
            .subscribe(move || {
                if let Some(owner) = weak.upgrade() {
                    owner.fetch_add(1, Ordering::SeqCst);
                }
            })
            .unwrap();
        assert_eq!(Arc::strong_count(&calls), 1);
        drop(calls);
        latch.arm(ShutdownReason::UserExit);
        assert!(latch.state.lock().callback.is_none());
    }
}
