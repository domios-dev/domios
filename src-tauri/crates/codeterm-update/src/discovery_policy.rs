use std::time::Duration;

pub const EVENT_THROTTLE: Duration = Duration::from_secs(60);
pub const INTERVAL: Duration = Duration::from_secs(30 * 60);
// One minute under the timer, so a 30-minute tick is never skipped by its own previous attempt.
pub const AUTOMATIC_CHECK_GAP: Duration = Duration::from_secs(29 * 60);

pub fn backoff(failures: u32) -> Duration {
    EVENT_THROTTLE
        .saturating_mul(2_u32.saturating_pow(failures.saturating_sub(1).min(5)))
        .min(INTERVAL)
}

/// A last attempt in the future means the clock moved; that must not silence checks.
pub fn automatic_check_due(last_attempt_ms: Option<u64>, now_ms: u64) -> bool {
    last_attempt_ms
        .and_then(|last| now_ms.checked_sub(last))
        .is_none_or(|age| age >= AUTOMATIC_CHECK_GAP.as_millis() as u64)
}
