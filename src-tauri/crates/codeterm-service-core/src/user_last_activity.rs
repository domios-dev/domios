use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const COALESCE_MS: u64 = 1000;

/// Coalesced daemon-wide last deliberate user activity (epoch ms).
pub struct UserLastActivity {
    last_ms: AtomicU64,
}

impl UserLastActivity {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            last_ms: AtomicU64::new(now_ms()),
        })
    }

    pub fn mark(&self) {
        let now = now_ms();
        let current = self.last_ms.load(Ordering::Relaxed);
        if now.saturating_sub(current) > COALESCE_MS {
            self.last_ms.store(now, Ordering::Relaxed);
        }
    }

    pub fn read_ms(&self) -> u64 {
        self.last_ms.load(Ordering::Relaxed)
    }

    pub fn read_for_gossip(&self) -> Option<u64> {
        if crate::paths::is_dev_instance() {
            None
        } else {
            Some(self.read_ms())
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn coalesces_writes_within_one_second() {
        let a = UserLastActivity::new();
        let t0 = a.read_ms();
        a.mark();
        let t1 = a.read_ms();
        assert!(t1 >= t0);
        thread::sleep(Duration::from_millis(50));
        a.mark();
        assert_eq!(
            a.read_ms(),
            t1,
            "second mark inside coalesce window must not advance"
        );
        thread::sleep(Duration::from_millis(1100));
        a.mark();
        assert!(a.read_ms() > t1, "mark after coalesce window must advance");
    }
}
