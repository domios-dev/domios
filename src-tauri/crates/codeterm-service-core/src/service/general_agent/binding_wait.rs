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

pub fn notify_binding_state_changed() {
    let signal = signal();
    let generation = {
        let mut generation = signal.generation.lock();
        *generation += 1;
        *generation
    };
    signal.changed.notify_all();
    signal.async_changed.send_replace(generation);
}

pub fn wait_for_session_binding<T>(
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

pub(crate) async fn wait_for_session_binding_async<T, Fut>(
    deadline: Instant,
    mut ready: impl FnMut() -> Fut,
) -> Option<T>
where
    Fut: std::future::Future<Output = Option<T>>,
{
    let mut changed = signal().async_changed.subscribe();
    loop {
        let _generation = *changed.borrow_and_update();
        if let Some(value) = ready().await {
            return Some(value);
        }
        if tokio::time::Instant::now() >= tokio::time::Instant::from_std(deadline) {
            return None;
        }
        if tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), changed.changed())
            .await
            .is_err()
        {
            return ready().await;
        }
    }
}

#[cfg(test)]
mod tests;
