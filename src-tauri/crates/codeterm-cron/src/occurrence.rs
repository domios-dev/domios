pub const LATE_FIRE_MS: u64 = 5_000;
pub const STALE_FIRE_MS: u64 = 10 * 60 * 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Occurrence {
    pub scheduled_at: u64,
    pub due_at: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FireDecision {
    WaitUntil(u64),
    Fire { late_by_ms: Option<u64> },
    Missed,
}

impl Occurrence {
    pub fn decision(self, now: u64, oneshot: bool, job_running: bool) -> FireDecision {
        if now < self.due_at {
            return FireDecision::WaitUntil(self.due_at);
        }
        let delay = now - self.due_at;
        if !oneshot && delay > STALE_FIRE_MS {
            return FireDecision::Missed;
        }
        if job_running {
            let recheck = now.saturating_add(30_000);
            let expiry = self.due_at.saturating_add(STALE_FIRE_MS + 1);
            return FireDecision::WaitUntil(if oneshot {
                recheck
            } else {
                recheck.min(expiry)
            });
        }
        FireDecision::Fire {
            late_by_ms: (delay > LATE_FIRE_MS).then_some(delay),
        }
    }
}

#[cfg(test)]
mod tests;
