//! Injectable time source for the cron subsystem.
//!
//! Everything cron-related asks "what time is it" through a `Clock` so
//! tests (and the gated /api/debug/clock endpoint) can time-travel instead
//! of sleeping wall-clock seconds. Production uses `SystemClock`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub trait Clock: Send + Sync + 'static {
    /// Milliseconds since the Unix epoch.
    fn now_ms(&self) -> u64;
}

pub type SharedClock = Arc<dyn Clock>;

#[derive(Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

pub fn system_clock() -> SharedClock {
    Arc::new(SystemClock)
}

/// Thread-safe manual clock. Starts at the real current time so cron
/// expressions ("next minute boundary") behave sanely, then moves only
/// when told to.
pub struct MockClock {
    now_ms: AtomicU64,
}

impl MockClock {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            now_ms: AtomicU64::new(SystemClock.now_ms()),
        })
    }

    pub fn starting_at(ms: u64) -> Arc<Self> {
        Arc::new(Self {
            now_ms: AtomicU64::new(ms),
        })
    }

    /// Move time forward; returns the new now.
    pub fn advance_ms(&self, delta_ms: u64) -> u64 {
        self.now_ms.fetch_add(delta_ms, Ordering::SeqCst) + delta_ms
    }

    pub fn set_ms(&self, ms: u64) {
        self.now_ms.store(ms, Ordering::SeqCst);
    }
}

impl Clock for MockClock {
    fn now_ms(&self) -> u64 {
        self.now_ms.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_clock_is_current() {
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let now = SystemClock.now_ms();
        assert!(now >= before && now < before + 10_000);
    }

    #[test]
    fn mock_clock_advances_and_sets() {
        let clock = MockClock::starting_at(1_000_000);
        assert_eq!(clock.now_ms(), 1_000_000);
        assert_eq!(clock.advance_ms(30_000), 1_030_000);
        assert_eq!(clock.now_ms(), 1_030_000);
        clock.set_ms(2_000_000);
        assert_eq!(clock.now_ms(), 2_000_000);
    }

    #[test]
    fn mock_clock_is_thread_safe() {
        let clock = MockClock::starting_at(0);
        let mut handles = Vec::new();
        for _ in 0..8 {
            let c = clock.clone();
            handles.push(std::thread::spawn(move || {
                for _ in 0..1000 {
                    c.advance_ms(1);
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(clock.now_ms(), 8_000);
    }
}
