use std::time::Duration;
use tokio::time::Instant;

#[derive(Default)]
pub(super) struct Failure {
    attempts: u32,
    retry_at: Option<Instant>,
    diagnostic: Option<String>,
}

impl Failure {
    pub fn record(&mut self, now: Instant, diagnostic: String) -> bool {
        let changed = self.diagnostic.as_ref() != Some(&diagnostic);
        if changed {
            log::warn!("[account-mesh] tailnet listener unavailable: {diagnostic}");
            self.diagnostic = Some(diagnostic);
        }
        let seconds = (30u64 << self.attempts.min(4)).min(300);
        self.attempts = self.attempts.saturating_add(1);
        self.retry_at = Some(now + Duration::from_secs(seconds));
        changed
    }

    pub fn due(&self, now: Instant) -> bool {
        self.retry_at.is_none_or(|at| now >= at)
    }

    pub fn retry_at(&self) -> Option<Instant> {
        self.retry_at
    }
}
