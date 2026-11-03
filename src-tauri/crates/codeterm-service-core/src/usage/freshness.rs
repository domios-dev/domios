use std::time::Duration;

pub(super) const MAX_AUTOMATIC_FAILURES: u32 = 4;

pub(super) fn refresh_due(
    active: bool,
    snapshot_age: Option<Duration>,
    attempt_age: Option<Duration>,
    in_flight: bool,
    failures: u32,
    idle_interval: Duration,
) -> bool {
    if in_flight || failures >= MAX_AUTOMATIC_FAILURES {
        return false;
    }
    let freshness = if active {
        Duration::from_secs(300).min(idle_interval)
    } else {
        idle_interval
    };
    let retry = if failures == 0 {
        freshness
    } else {
        Duration::from_secs(60 * (1_u64 << failures.saturating_sub(1).min(3))).min(freshness)
    };
    if attempt_age.is_some_and(|age| age < retry) {
        return false;
    }
    failures > 0 || snapshot_age.is_none_or(|age| age >= freshness)
}

#[cfg(test)]
mod tests;
