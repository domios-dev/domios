use parking_lot::{Condvar, Mutex};
use std::sync::OnceLock;
use std::time::Instant;

struct BootSignal {
    generation: Mutex<u64>,
    changed: Condvar,
}

fn signal() -> &'static BootSignal {
    static SIGNAL: OnceLock<BootSignal> = OnceLock::new();
    SIGNAL.get_or_init(|| BootSignal {
        generation: Mutex::new(0),
        changed: Condvar::new(),
    })
}

pub fn notify_boot_changed() {
    let signal = signal();
    *signal.generation.lock() += 1;
    signal.changed.notify_all();
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
