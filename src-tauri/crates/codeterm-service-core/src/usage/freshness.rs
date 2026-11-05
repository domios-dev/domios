use std::time::Duration;

pub(super) const MAX_AUTOMATIC_FAILURES: u32 = 4;
/// After repeated failures the probe slows down instead of stopping, so a
/// provider that recovers is picked up again without a manual refresh.
const PERSISTENT_FAILURE_RETRY: Duration = Duration::from_secs(1800);

pub(super) fn refresh_due(
    active: bool,
    snapshot_age: Option<Duration>,
    attempt_age: Option<Duration>,
    in_flight: bool,
    failures: u32,
    idle_interval: Duration,
) -> bool {
    if in_flight {
        return false;
    }
    if failures >= MAX_AUTOMATIC_FAILURES {
        return attempt_age.is_none_or(|age| age >= PERSISTENT_FAILURE_RETRY);
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
