use parking_lot::{Condvar, Mutex};
use std::sync::{Arc, OnceLock, Weak};
use std::time::Instant;

struct BootSignal {
    generation: Mutex<u64>,
    changed: Condvar,
    observers: Mutex<Vec<Weak<crate::pty::screen_state::ScreenChangeSignal>>>,
}

fn signal() -> &'static BootSignal {
    static SIGNAL: OnceLock<BootSignal> = OnceLock::new();
    SIGNAL.get_or_init(|| BootSignal {
        generation: Mutex::new(0),
        changed: Condvar::new(),
        observers: Mutex::new(Vec::new()),
    })
}

pub fn notify_boot_changed() {
    let signal = signal();
    *signal.generation.lock() += 1;
    signal.changed.notify_all();
    let observers = {
        let mut observers = signal.observers.lock();
        observers.retain(|observer| observer.strong_count() > 0);
        observers
            .iter()
            .filter_map(Weak::upgrade)
            .collect::<Vec<_>>()
    };
    for observer in observers {
        observer.mark_changed();
    }
}

pub(super) fn observe_boot_changes(observer: &Arc<crate::pty::screen_state::ScreenChangeSignal>) {
    signal().observers.lock().push(Arc::downgrade(observer));
}

pub fn wait_until(deadline: Instant, mut ready: impl FnMut() -> bool) -> bool {
    let signal = signal();
    loop {
        let seen = *signal.generation.lock();
        if ready() {
            return true;
        }
        let now = Instant::now();
        if now >= deadline {
            return false;
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

    #[test]
    fn lifecycle_change_wakes_command_observer_without_delay() {
        let observer = Arc::new(crate::pty::screen_state::ScreenChangeSignal::default());
        observe_boot_changes(&observer);
        let generation = observer.generation();
        notify_boot_changed();
        assert!(observer.wait_after(generation, std::time::Duration::ZERO));
    }
}
