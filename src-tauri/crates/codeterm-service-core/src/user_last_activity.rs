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

    #[test]
    fn coalesces_writes_within_one_second() {
        let a = UserLastActivity::new();
        let t0 = now_ms();
        a.last_ms.store(t0, Ordering::Relaxed);
        a.mark();
        let t1 = a.read_ms();
        assert_eq!(t1, t0, "mark inside coalesce window must not advance");

        let expired = now_ms().saturating_sub(COALESCE_MS + 1);
        a.last_ms.store(expired, Ordering::Relaxed);
        a.mark();
        assert!(
            a.read_ms() > expired,
            "mark after coalesce window must advance"
        );
    }
}
