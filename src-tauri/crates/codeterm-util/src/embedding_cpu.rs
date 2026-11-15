use std::sync::atomic::{AtomicBool, Ordering};

pub const DEFAULT_THREADS: &str = "2";

static DEFAULT_INJECTED: AtomicBool = AtomicBool::new(false);

pub fn record_default_injection() {
    DEFAULT_INJECTED.store(true, Ordering::Release);
}

pub fn default_was_injected() -> bool {
    DEFAULT_INJECTED.load(Ordering::Acquire)
}
