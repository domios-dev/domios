use parking_lot::{Mutex, MutexGuard};

#[track_caller]
pub(super) fn lock<'a, T>(mutex: &'a Mutex<T>, name: &'static str) -> MutexGuard<'a, T> {
    #[cfg(debug_assertions)]
    let started = std::time::Instant::now();
    let guard = mutex.lock();
    #[cfg(debug_assertions)]
    if started.elapsed().as_millis() >= 50 {
        log::debug!(
            "[STREAM-MUX-LOCK] {} wait={}ms at {} thread={:?}",
            name,
            started.elapsed().as_millis(),
            std::panic::Location::caller(),
            std::thread::current().id()
        );
    }
    #[cfg(not(debug_assertions))]
    let _ = name;
    guard
}

pub(super) fn cleanup(work: impl FnOnce() + Send + 'static) {
    if let Ok(runtime) = tokio::runtime::Handle::try_current() {
        runtime.spawn_blocking(work);
    } else {
        work();
    }
}
