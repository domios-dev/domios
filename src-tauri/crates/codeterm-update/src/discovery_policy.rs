use std::time::Duration;

pub const EVENT_THROTTLE: Duration = Duration::from_secs(60);
pub const INTERVAL: Duration = Duration::from_secs(30 * 60);

pub fn backoff(failures: u32) -> Duration {
    EVENT_THROTTLE
        .saturating_mul(2_u32.saturating_pow(failures.saturating_sub(1).min(5)))
        .min(INTERVAL)
}
