use std::collections::HashMap;
use std::time::{Duration, Instant};

const RETRY_AFTER: Duration = Duration::from_secs(60);

struct Outage {
    epoch: u64,
    retry_at: Instant,
}

#[derive(Default)]
pub(super) struct Backoffs(HashMap<String, Outage>);

impl Backoffs {
    pub(super) fn available(&self, adapter: &str, epoch: u64, now: Instant) -> bool {
        self.0
            .get(adapter)
            .is_none_or(|outage| outage.epoch != epoch || now >= outage.retry_at)
    }

    pub(super) fn failed(&mut self, adapter: &str, epoch: u64, now: Instant) -> bool {
        let first = self
            .0
            .get(adapter)
            .is_none_or(|outage| outage.epoch != epoch);
        self.0.insert(
            adapter.into(),
            Outage {
                epoch,
                retry_at: now + RETRY_AFTER,
            },
        );
        first
    }

    pub(super) fn recovered(&mut self, adapter: &str) {
        self.0.remove(adapter);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outage_records_once_and_only_probes_after_backoff_or_connection_change() {
        let now = Instant::now();
        let mut backoffs = Backoffs::default();
        assert!(backoffs.available("adapter", 0, now));
        assert!(backoffs.failed("adapter", 0, now));
        assert!(!backoffs.available("adapter", 0, now + Duration::from_secs(59)));
        assert!(backoffs.available("adapter", 0, now + RETRY_AFTER));
        assert!(!backoffs.failed("adapter", 0, now + RETRY_AFTER));
        assert!(!backoffs.available("adapter", 0, now + RETRY_AFTER));
        assert!(backoffs.available("adapter", 1, now));
        assert!(backoffs.failed("adapter", 1, now));
        backoffs.recovered("adapter");
        assert!(backoffs.available("adapter", 1, now));
        assert!(backoffs.failed("adapter", 1, now));
    }
}
