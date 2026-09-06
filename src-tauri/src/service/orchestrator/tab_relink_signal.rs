// Event boundary for "a Tab's pty_id was (re)linked". Replaces the 100ms poll in
// `await_replacement_pty`: linkers bump a generation and wake waiters; a waiter

use parking_lot::{Condvar, Mutex};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

struct RelinkSignal {
    generation: Mutex<u64>,
    changed: Condvar,
}

fn signal() -> &'static RelinkSignal {
    static SIGNAL: OnceLock<RelinkSignal> = OnceLock::new();
    SIGNAL.get_or_init(|| RelinkSignal {
        generation: Mutex::new(0),
        changed: Condvar::new(),
    })
}

/// Wake anyone awaiting a tab relink. Called after every `link_pty`.
pub fn notify_tab_relinked() {
    let sig = signal();
    *sig.generation.lock() += 1;
    sig.changed.notify_all();
}

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
