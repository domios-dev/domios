use parking_lot::{Condvar, Mutex};
use std::sync::OnceLock;
use std::time::Instant;

struct BindingSignal {
    generation: Mutex<u64>,
    changed: Condvar,
}

fn signal() -> &'static BindingSignal {
    static SIGNAL: OnceLock<BindingSignal> = OnceLock::new();
    SIGNAL.get_or_init(|| BindingSignal {
        generation: Mutex::new(0),
        changed: Condvar::new(),
    })
}

pub(crate) fn notify_session_bound() {
    let signal = signal();
    *signal.generation.lock() += 1;
    signal.changed.notify_all();
}

pub(crate) fn wait_for_session_binding<T>(
    deadline: Instant,
    mut ready: impl FnMut() -> Option<T>,
) -> Option<T> {
    let signal = signal();
    loop {
        let seen = *signal.generation.lock();
        if let Some(value) = ready() {
            return Some(value);
        }
        let now = Instant::now();
        if now >= deadline {
            return None;
        }
        let mut generation = signal.generation.lock();
        if *generation == seen {
            signal.changed.wait_for(&mut generation, deadline - now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn binding_notification_releases_waiter_without_polling() {
        let bound = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&bound);
        let (ready_tx, ready_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let waiter = std::thread::spawn(move || {
            let value = wait_for_session_binding(Instant::now() + Duration::from_secs(1), || {
                ready_tx.send(()).ok();
                observed.load(Ordering::SeqCst).then_some("session-id")
            });
            done_tx.send(value).unwrap();
        });

        ready_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("waiter must inspect the binding before waiting");
        bound.store(true, Ordering::SeqCst);
        notify_session_bound();

        assert_eq!(
            done_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("binding event must release the waiter"),
            Some("session-id")
        );
        waiter.join().unwrap();
    }
}
