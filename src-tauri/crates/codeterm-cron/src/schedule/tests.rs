use super::*;
mod compatibility;

#[test]
fn jitter_default_is_ten_percent_of_period() {
    use chrono::TimeZone;
    let now = chrono::Utc
        .with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
        .single()
        .unwrap()
        .timestamp_millis() as u64;
    let schedule = "* * * * *";
    let plain = compute_next_run_at(schedule, None, now).unwrap().unwrap();
    let jittered = compute_next_run_jittered_at(schedule, None, None, now)
        .unwrap()
        .unwrap();

    assert!(jittered >= plain);
    assert!(jittered <= plain + 6_000);
}

#[test]
fn jitter_zero_is_no_op() {
    let plain = compute_next_run("0 * * * *").unwrap().unwrap();
    let jittered = compute_next_run_jittered("0 * * * *", Some(0))
        .unwrap()
        .unwrap();
    assert_eq!(plain, jittered);
}

#[test]
fn jitter_explicit_max_respected() {
    let plain = compute_next_run("0 * * * *").unwrap().unwrap();
    for _ in 0..10 {
        let j = compute_next_run_jittered("0 * * * *", Some(5_000))
            .unwrap()
            .unwrap();
        assert!(j >= plain && j <= plain + 5_000);
    }
}

#[test]
fn backoff_grows_exponentially_capped_at_hour() {
    let base = 60_000u64;
    let b1 = compute_backoff(1, base);
    let b2 = compute_backoff(2, base);
    let b5 = compute_backoff(5, base);
    assert!(b1 < b2);
    assert!(b5 <= 3_600_000 + (3_600_000 / 4));
}

#[test]
fn test_next_run_computation() {
    let next = compute_next_run("*/30 * * * *").unwrap();
    assert!(next.is_some());
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    assert!(next.unwrap() > now);
    assert!(next.unwrap() - now <= 30 * 60 * 1000 + 1000); // +1s tolerance
}

#[test]
fn test_5field_to_6field() {
    assert_eq!(to_6field("*/5 * * * *"), "0 */5 * * * *");
    assert_eq!(to_6field("0 9 * * 1"), "0 0 9 * * 1");
}

#[test]
fn test_invalid_schedule() {
    assert!(validate_schedule("not a cron").is_err());
    assert!(validate_schedule("* * *").is_err());
    assert!(validate_schedule("0 9 * * *").is_ok());
}

#[test]
fn cron_schedule_validation_table() {
    use chrono::TimeZone;
    let now = chrono::Utc
        .with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
        .single()
        .unwrap()
        .timestamp_millis() as u64;
    let cases = [
        ("every minute", "* * * * *", true),
        ("annual", "0 0 1 1 *", true),
        ("leap day", "0 0 29 2 *", true),
        ("seconds field", "*/1 * * * * *", false),
        ("impossible date", "0 0 30 2 *", false),
        ("oneshot without mode", "in 2h", false),
    ];

    for (label, schedule, accepted) in cases {
        let result = compute_next_run_at(schedule, None, now);
        assert_eq!(result.is_ok(), accepted, "{label}: {result:?}");
        if !accepted {
            assert!(result.unwrap_err().starts_with("schedule:"), "{label}");
        }
    }
}

#[test]
fn cron_jitter_is_capped_to_half_the_schedule_period() {
    use chrono::TimeZone;
    let now = chrono::Utc
        .with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
        .single()
        .unwrap()
        .timestamp_millis() as u64;
    let schedule = "* * * * *";
    let scheduled = compute_next_run_at(schedule, None, now).unwrap().unwrap();
    let maximum = compute_next_run_jittered_at(schedule, Some(u64::MAX), None, now)
        .unwrap()
        .unwrap();

    assert!(maximum >= scheduled);
    assert!(maximum <= scheduled + 30_000);
}

#[test]
fn test_parse_oneshot_relative() {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    let ts = parse_oneshot_schedule("in 5m").unwrap();
    assert!(ts > now);
    assert!(ts - now >= 4 * 60_000);
    assert!(ts - now <= 6 * 60_000);

    let ts2 = parse_oneshot_schedule("in 1h30m").unwrap();
    assert!(ts2 - now >= 89 * 60_000);
    assert!(ts2 - now <= 91 * 60_000);

    let ts3 = parse_oneshot_schedule("in 2d").unwrap();
    assert!(ts3 - now >= 47 * 3_600_000);
}

#[test]
fn test_parse_oneshot_iso8601() {
    let future = chrono::Utc::now() + chrono::Duration::hours(2);
    let iso = future.to_rfc3339();
    let ts = parse_oneshot_schedule(&iso).unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    assert!(ts > now);
}

#[test]
fn test_parse_oneshot_invalid() {
    assert!(parse_oneshot_schedule("not a time").is_err());
    assert!(parse_oneshot_schedule("in 0m").is_err());
}

#[test]
fn tz_none_matches_utc() {
    let utc = compute_next_run("0 3 * * *").unwrap();
    let none = compute_next_run_in("0 3 * * *", None).unwrap();
    let empty = compute_next_run_in("0 3 * * *", Some("")).unwrap();
    assert_eq!(utc, none);
    assert_eq!(utc, empty);
}

#[test]
fn tz_riga_shifts_from_utc() {
    // two MUST differ by 2-3 hours, never zero.
    let utc = compute_next_run_in("0 3 * * *", None).unwrap().unwrap();
    let riga = compute_next_run_in("0 3 * * *", Some("Europe/Riga"))
        .unwrap()
        .unwrap();
    let diff_ms = (utc as i64 - riga as i64).abs();
    // Could be the same calendar day or different — both legitimate. The shift between them mod 24h is 2-3 hours.
    let day_ms: i64 = 24 * 60 * 60 * 1000;
    let mod_diff = diff_ms.rem_euclid(day_ms);
    assert!(
        mod_diff == 2 * 3_600_000
            || mod_diff == 3 * 3_600_000
            || (day_ms - mod_diff) == 2 * 3_600_000
            || (day_ms - mod_diff) == 3 * 3_600_000,
        "expected ±2 or ±3h shift between UTC and Europe/Riga, got mod_diff={}",
        mod_diff
    );
}

#[test]
fn tz_invalid_string_errors() {
    assert!(compute_next_run_in("0 3 * * *", Some("Not/A_Zone")).is_err());
    assert!(validate_tz("Not/A_Zone").is_err());
    assert!(validate_tz("Europe/Riga").is_ok());
    assert!(validate_tz("UTC").is_ok());
}

#[test]
fn tz_jittered_uses_tz_for_base_fire() {
    let utc = compute_next_run_jittered_in("0 3 * * *", Some(0), None)
        .unwrap()
        .unwrap();
    let riga = compute_next_run_jittered_in("0 3 * * *", Some(0), Some("Europe/Riga"))
        .unwrap()
        .unwrap();
    // Jitter=0 → deterministic; tz shift must show up.
    assert_ne!(utc, riga);
}

#[test]
fn tz_invalid_via_jittered_errors() {
    assert!(compute_next_run_jittered_in("0 3 * * *", None, Some("Mars/Time")).is_err());
}

#[test]
fn stale_fire_window_applies_only_to_recurring_jobs() {
    let slot = 1_700_000_000_000;
    assert!(!is_stale_fire(slot, slot + STALE_FIRE_MS, false));
    assert!(is_stale_fire(slot, slot + STALE_FIRE_MS + 1, false));
    assert!(!is_stale_fire(slot, slot + STALE_FIRE_MS * 6, true));
    assert!(!is_stale_fire(slot + 5, slot, false));
}
