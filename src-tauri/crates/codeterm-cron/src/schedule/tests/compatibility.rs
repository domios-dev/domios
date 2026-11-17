use super::super::*;

fn millis(value: &str) -> u64 {
    chrono::DateTime::parse_from_rfc3339(value)
        .unwrap()
        .timestamp_millis() as u64
}

#[test]
fn stored_five_field_schedules_keep_exact_fire_times() {
    let cases = [
        (
            "* * * * *",
            None,
            "2026-01-01T00:00:00Z",
            "2026-01-01T00:01:00Z",
            "2026-01-01T00:02:00Z",
        ),
        (
            "*/15 * * * *",
            None,
            "2026-01-01T00:14:59Z",
            "2026-01-01T00:15:00Z",
            "2026-01-01T00:30:00Z",
        ),
        (
            "5,35 9-10 * * *",
            None,
            "2026-01-01T09:05:00Z",
            "2026-01-01T09:35:00Z",
            "2026-01-01T10:05:00Z",
        ),
        (
            "0 0 1 1 *",
            None,
            "2026-12-31T23:59:59Z",
            "2027-01-01T00:00:00Z",
            "2028-01-01T00:00:00Z",
        ),
        (
            "0 0 29 2 *",
            None,
            "2026-01-01T00:00:00Z",
            "2028-02-29T00:00:00Z",
            "2032-02-29T00:00:00Z",
        ),
        (
            "0 9 * * MON-FRI",
            None,
            "2026-01-02T09:00:00Z",
            "2026-01-05T09:00:00Z",
            "2026-01-06T09:00:00Z",
        ),
        (
            "0 9 * * 1",
            None,
            "2026-01-03T10:00:00Z",
            "2026-01-04T09:00:00Z",
            "2026-01-11T09:00:00Z",
        ),
        (
            "0 9 * * 7",
            None,
            "2026-01-02T10:00:00Z",
            "2026-01-03T09:00:00Z",
            "2026-01-10T09:00:00Z",
        ),
        (
            "0 9 1 * MON",
            None,
            "2026-01-01T00:00:00Z",
            "2026-06-01T09:00:00Z",
            "2027-02-01T09:00:00Z",
        ),
        (
            "0 9 * JAN MON",
            None,
            "2026-01-01T00:00:00Z",
            "2026-01-05T09:00:00Z",
            "2026-01-12T09:00:00Z",
        ),
        (
            "0 9 * * *",
            Some("Asia/Kathmandu"),
            "2026-01-01T00:00:00Z",
            "2026-01-01T03:15:00Z",
            "2026-01-02T03:15:00Z",
        ),
        (
            "0 9 * * *",
            Some("Europe/Riga"),
            "2026-01-01T00:00:00Z",
            "2026-01-01T07:00:00Z",
            "2026-01-02T07:00:00Z",
        ),
        (
            "0 9 * * *",
            Some("Europe/Riga"),
            "2026-07-01T00:00:00Z",
            "2026-07-01T06:00:00Z",
            "2026-07-02T06:00:00Z",
        ),
    ];
    for (schedule, tz, now, first, second) in cases {
        assert_eq!(
            upcoming_fires(schedule, tz, millis(now)),
            Ok((millis(first), Some(millis(second)))),
            "{schedule} in {tz:?} after {now}"
        );
        assert_eq!(
            compute_next_run_jittered_at(schedule, Some(0), tz, millis(now)),
            Ok(Some(millis(first)))
        );
    }
}

#[test]
fn timezone_gaps_and_overlaps_keep_both_fire_times() {
    let cases = [
        (
            "30 3 * * *",
            "Europe/Riga",
            "2026-03-28T02:00:00Z",
            "2026-03-30T00:30:00Z",
            "2026-03-31T00:30:00Z",
        ),
        (
            "30 3 * * *",
            "Europe/Riga",
            "2026-10-24T23:00:00Z",
            "2026-10-25T00:30:00Z",
            "2026-10-25T01:30:00Z",
        ),
        (
            "30 2 * * *",
            "America/New_York",
            "2026-03-07T08:00:00Z",
            "2026-03-09T06:30:00Z",
            "2026-03-10T06:30:00Z",
        ),
        (
            "30 1 * * *",
            "America/New_York",
            "2026-11-01T04:00:00Z",
            "2026-11-01T05:30:00Z",
            "2026-11-01T06:30:00Z",
        ),
    ];
    for (schedule, tz, now, first, second) in cases {
        assert_eq!(
            upcoming_fires(schedule, Some(tz), millis(now)),
            Ok((millis(first), Some(millis(second)))),
            "{schedule} in {tz} after {now}"
        );
    }
}

#[test]
fn invalid_stored_schedules_remain_refused() {
    for schedule in [
        "",
        "* * * *",
        "0 * * * * *",
        "*/0 * * * *",
        "60 * * * *",
        "0 24 * * *",
        "0 0 0 * *",
        "0 0 * 13 *",
        "0 0 * * 0",
        "0 0 * * 8",
        "0 0 30 2 *",
        "in 5m",
    ] {
        assert!(
            upcoming_fires(schedule, None, millis("2026-01-01T00:00:00Z")).is_err(),
            "{schedule}"
        );
    }
}
