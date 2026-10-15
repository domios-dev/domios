// Evaluate the wait predicate outside the lock so the condvar path never holds it across mux/pty acquisition.

use parking_lot::{Condvar, Mutex};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

struct RelinkSignal {
    generation: Mutex<u64>,
    changed: Condvar,
    linked: tokio::sync::Notify,
}

fn signal() -> &'static RelinkSignal {
    static SIGNAL: OnceLock<RelinkSignal> = OnceLock::new();
    SIGNAL.get_or_init(|| RelinkSignal {
        generation: Mutex::new(0),
        changed: Condvar::new(),
        linked: tokio::sync::Notify::new(),
    })
}

/// Wake anyone awaiting a tab relink. Called after every `link_pty`.
pub fn notify_tab_relinked() {
    let sig = signal();
    *sig.generation.lock() += 1;
    sig.changed.notify_all();
    sig.linked.notify_waiters();
}

pub async fn await_relink<T>(mut ready: impl FnMut() -> Option<T>) -> T {
    loop {
        let linked = signal().linked.notified();
        tokio::pin!(linked);
        linked.as_mut().enable();
        if let Some(value) = ready() {
            return value;
        }
        linked.await;
    }
}

pub async fn await_relink_async<T, F: std::future::Future<Output = Option<T>>>(
    mut ready: impl FnMut() -> F,
) -> T {
    loop {
        let linked = signal().linked.notified();
        tokio::pin!(linked);
        linked.as_mut().enable();
        if let Some(value) = ready().await {
            return value;
        }
        linked.await;
    }
}

#[cfg(test)]
mod tests;

/// Block until `ready()` returns `Some`, or `deadline` passes. Returns the ready
/// value, or `None` on timeout. `ready` is checked before each wait and is never
/// evaluated while the internal lock is held.
pub fn wait_for_relink<T>(deadline: Instant, mut ready: impl FnMut() -> Option<T>) -> Option<T> {
    let sig = signal();
    loop {
        let seen = *sig.generation.lock();
        if let Some(value) = ready() {
            return Some(value);
        }
        let now = Instant::now();
        if now >= deadline {
            return None;
        }
        let mut generation = sig.generation.lock();
        if *generation == seen {
            // No link landed between the predicate check and here; sleep until one
            // does or the deadline fires. Spurious wakeups just re-check the predicate.
            sig.changed.wait_for(
                &mut generation,
                (deadline - now).min(Duration::from_secs(30)),
            );
        }
    }
}
