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
        Some(Duration::from_secs(239)),
        false,
        3,
        Duration::from_secs(3000)
    ));
    assert!(refresh_due(
        true,
        None,
        Some(Duration::from_secs(300)),
        false,
        3,
        Duration::from_secs(3000)
    ));
}

#[test]
fn persistent_failure_slows_down_instead_of_stopping() {
    for active in [false, true] {
        for failures in [4, 16, u32::MAX] {
            assert!(!refresh_due(
                active,
                None,
                Some(Duration::from_secs(1799)),
                false,
                failures,
                Duration::from_secs(3000)
            ));
            assert!(refresh_due(
                active,
                None,
                Some(Duration::from_secs(1800)),
                false,
                failures,
                Duration::from_secs(3000)
            ));
        }
    }
}
