use parking_lot::{Condvar, Mutex};
use std::sync::OnceLock;
use std::time::Instant;
use tokio::sync::watch;

struct BindingSignal {
    generation: Mutex<u64>,
    changed: Condvar,
    async_changed: watch::Sender<u64>,
}

fn signal() -> &'static BindingSignal {
    static SIGNAL: OnceLock<BindingSignal> = OnceLock::new();
    SIGNAL.get_or_init(|| {
        let (async_changed, _) = watch::channel(0);
        BindingSignal {
            generation: Mutex::new(0),
            changed: Condvar::new(),
            async_changed,
        }
    })
}

pub(crate) fn notify_binding_state_changed() {
    let signal = signal();
    let generation = {
        let mut generation = signal.generation.lock();
        *generation += 1;
        *generation
    };
    signal.changed.notify_all();
    signal.async_changed.send_replace(generation);
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

pub(crate) async fn wait_for_session_binding_async<T>(
    deadline: Instant,
    mut ready: impl FnMut() -> Option<T>,
) -> Option<T> {
    let mut changed = signal().async_changed.subscribe();
    loop {
        let _generation = *changed.borrow_and_update();
        if let Some(value) = ready() {
            return Some(value);
        }
        if tokio::time::Instant::now() >= tokio::time::Instant::from_std(deadline) {
            return None;
        }
        if tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), changed.changed())
            .await
            .is_err()
        {
            return ready();
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
        notify_binding_state_changed();

        assert_eq!(
            done_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("binding event must release the waiter"),
            Some("session-id")
        );
        waiter.join().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn async_binding_wait_leaves_the_runtime_available_for_other_requests() {
        use std::sync::atomic::Ordering;

        let bound = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let observed = std::sync::Arc::clone(&bound);
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let waiter = tokio::spawn(async move {
            let mut ready_tx = Some(ready_tx);
            wait_for_session_binding_async(
                Instant::now() + std::time::Duration::from_secs(1),
                || {
                    if let Some(ready_tx) = ready_tx.take() {
                        let _ = ready_tx.send(());
                    }
                    observed.load(Ordering::SeqCst).then_some("bound")
                },
            )
            .await
        });

        ready_rx
            .await
            .expect("waiter must inspect state before waiting");
        let other_request = tokio::spawn(async { "completed" });
        assert_eq!(other_request.await.unwrap(), "completed");
        bound.store(true, Ordering::SeqCst);
        notify_binding_state_changed();
        assert_eq!(waiter.await.unwrap(), Some("bound"));
    }
}
