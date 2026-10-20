use std::time::Duration;

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
mod tests {
    use super::*;

    #[test]
    fn active_work_overrides_long_idle_deadline() {
        assert!(refresh_due(
            true,
            Some(Duration::from_secs(301)),
            Some(Duration::from_secs(301)),
            false,
            0,
            Duration::from_secs(3000)
        ));
        assert!(!refresh_due(
            false,
            Some(Duration::from_secs(301)),
            Some(Duration::from_secs(301)),
            false,
            0,
            Duration::from_secs(3000)
        ));
    }

    #[test]
    fn provider_events_keep_fresh_data_without_another_read() {
        assert!(!refresh_due(
            true,
            Some(Duration::from_secs(1)),
            Some(Duration::from_secs(600)),
            false,
            0,
            Duration::from_secs(3000)
        ));
        assert!(refresh_due(
            true,
            None,
            None,
            false,
            0,
            Duration::from_secs(3000)
        ));
    }

    #[test]
    fn in_flight_is_single_flight_even_when_overdue() {
        assert!(!refresh_due(
            true,
            None,
            None,
            true,
            0,
            Duration::from_secs(3000)
        ));
    }

    #[test]
    fn failure_backoff_is_bounded_during_active_work() {
        assert!(!refresh_due(
            true,
            None,
            Some(Duration::from_secs(59)),
            false,
            1,
            Duration::from_secs(3000)
        ));
        assert!(refresh_due(
            true,
            None,
            Some(Duration::from_secs(60)),
            false,
            1,
            Duration::from_secs(3000)
        ));
        assert!(!refresh_due(
            true,
            None,
            Some(Duration::from_secs(299)),
            false,
            10,
            Duration::from_secs(3000)
        ));
        assert!(refresh_due(
            true,
            None,
            Some(Duration::from_secs(300)),
            false,
            10,
            Duration::from_secs(3000)
        ));
    }
}
