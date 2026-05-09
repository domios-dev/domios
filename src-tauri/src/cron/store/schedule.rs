//! Schedule parsing, validation, and next-fire-time computation.
//!
//! Pure functions — no state, no I/O. Used by both [`super::CronStore`] and
//! external callers (cron::scheduler tests, etc.) via the parent's `pub use`.

use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_CONSECUTIVE_FAILURES: u32 = 5;

pub fn to_6field(expr: &str) -> String {
    let fields: Vec<&str> = expr.trim().split_whitespace().collect();
    if fields.len() == 5 {
        format!("0 {}", expr.trim())
    } else {
        expr.trim().to_string()
    }
}

pub fn parse_tz(tz: Option<&str>) -> Result<Option<chrono_tz::Tz>, String> {
    match tz {
        None => Ok(None),
        Some(s) if s.trim().is_empty() => Ok(None),
        Some(s) => s
            .parse::<chrono_tz::Tz>()
            .map(Some)
            .map_err(|e| format!("Invalid timezone '{}': {}", s, e)),
    }
}

pub fn compute_next_run(schedule: &str) -> Result<Option<u64>, String> {
    compute_next_run_in(schedule, None)
}

pub fn compute_next_run_in(
    schedule: &str,
    tz: Option<&str>,
) -> Result<Option<u64>, String> {
    use std::str::FromStr;
    let expr = to_6field(schedule);
    let sched = cron::Schedule::from_str(&expr).map_err(|e| format!("Invalid cron: {}", e))?;
    let parsed_tz = parse_tz(tz)?;
    Ok(match parsed_tz {
        Some(tz) => sched.upcoming(tz).next().map(|dt| dt.timestamp_millis() as u64),
        None => sched.upcoming(chrono::Utc).next().map(|dt| dt.timestamp_millis() as u64),
    })
}

pub fn compute_next_run_jittered(
    schedule: &str,
    jitter_max_ms: Option<u64>,
) -> Result<Option<u64>, String> {
    compute_next_run_jittered_in(schedule, jitter_max_ms, None)
}

pub fn compute_next_run_jittered_in(
    schedule: &str,
    jitter_max_ms: Option<u64>,
    tz: Option<&str>,
) -> Result<Option<u64>, String> {
    use std::str::FromStr;
    let expr = to_6field(schedule);
    let sched = cron::Schedule::from_str(&expr).map_err(|e| format!("Invalid cron: {}", e))?;
    let parsed_tz = parse_tz(tz)?;
    let (next, second) = match parsed_tz {
        Some(tz) => {
            let mut upcoming = sched.upcoming(tz);
            let n = match upcoming.next() {
                Some(dt) => dt.timestamp_millis() as u64,
                None => return Ok(None),
            };
            let s = upcoming.next().map(|dt| dt.timestamp_millis() as u64);
            (n, s)
        }
        None => {
            let mut upcoming = sched.upcoming(chrono::Utc);
            let n = match upcoming.next() {
                Some(dt) => dt.timestamp_millis() as u64,
                None => return Ok(None),
            };
            let s = upcoming.next().map(|dt| dt.timestamp_millis() as u64);
            (n, s)
        }
    };

    let max = match jitter_max_ms {
        Some(0) => return Ok(Some(next)),
        Some(n) => n,
        None => {
            let interval_ms = second.map(|s| s.saturating_sub(next)).unwrap_or(0);
            let raw = interval_ms / 10;
            raw.clamp(60_000, 1_800_000)
        }
    };
    if max == 0 {
        return Ok(Some(next));
    }
    use rand::Rng;
    let offset = rand::thread_rng().gen_range(0..=max);
    Ok(Some(next + offset))
}

pub fn compute_backoff(consecutive_failures: u32, base_ms: u64) -> u64 {
    let factor = 1u64.checked_shl(consecutive_failures.min(10)).unwrap_or(1024);
    let raw = base_ms.saturating_mul(factor);
    let capped = raw.min(3_600_000);
    use rand::Rng;
    let jitter = (capped as f64 * 0.25 * rand::thread_rng().gen::<f64>()) as u64;
    capped + jitter
}

pub fn validate_schedule(schedule: &str) -> Result<(), String> {
    use std::str::FromStr;
    let expr = to_6field(schedule);
    cron::Schedule::from_str(&expr).map_err(|e| format!("Invalid cron: {}", e))?;
    Ok(())
}

pub fn validate_tz(tz: &str) -> Result<(), String> {
    parse_tz(Some(tz)).map(|_| ())
}

/// Parse a oneshot schedule string into an absolute Unix timestamp (ms).
///
/// Accepts:
/// - Relative: "in 5m", "in 1h30m", "in 2h", "in 90s", "in 1d"
/// - ISO 8601: "2025-04-05T14:30:00Z", "2025-04-05T14:30:00+03:00"
/// - Unix ms: "1712345678000" (13+ digits)
pub fn parse_oneshot_schedule(schedule: &str) -> Result<u64, String> {
    let s = schedule.trim();

    if let Some(rest) = s.strip_prefix("in ") {
        let duration_ms = parse_relative_duration(rest.trim())?;
        if duration_ms == 0 {
            return Err("Duration must be > 0".into());
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        return Ok(now + duration_ms);
    }

    if s.len() >= 13 && s.chars().all(|c| c.is_ascii_digit()) {
        let ts: u64 = s.parse().map_err(|_| "Invalid timestamp".to_string())?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        if ts <= now {
            return Err("Oneshot time must be in the future".into());
        }
        return Ok(ts);
    }

    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        let ts = dt.timestamp_millis() as u64;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        if ts <= now {
            return Err("Oneshot time must be in the future".into());
        }
        return Ok(ts);
    }

    Err(format!(
        "Invalid oneshot schedule '{}'. Use: 'in 5m', 'in 1h30m', ISO8601, or unix ms",
        s
    ))
}

fn parse_relative_duration(s: &str) -> Result<u64, String> {
    let mut total_ms: u64 = 0;
    let mut num_buf = String::new();

    for c in s.chars() {
        if c.is_ascii_digit() {
            num_buf.push(c);
        } else {
            let n: u64 = num_buf.parse().map_err(|_| format!("Invalid number in duration: '{}'", s))?;
            num_buf.clear();
            match c {
                's' => total_ms += n * 1_000,
                'm' => total_ms += n * 60_000,
                'h' => total_ms += n * 3_600_000,
                'd' => total_ms += n * 86_400_000,
                _ => return Err(format!("Unknown duration unit '{}' in '{}'", c, s)),
            }
        }
    }

    if !num_buf.is_empty() {
        // bare number = minutes
        let n: u64 = num_buf.parse().map_err(|_| format!("Invalid number in duration: '{}'", s))?;
        total_ms += n * 60_000;
    }

    if total_ms == 0 {
        return Err(format!("Could not parse duration from '{}'", s));
    }

    Ok(total_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jitter_default_is_within_clamp() {
        let plain = compute_next_run("0 * * * *").unwrap().unwrap();
        let jittered = compute_next_run_jittered("0 * * * *", None).unwrap().unwrap();
        assert!(jittered >= plain, "jitter never goes backwards");
        assert!(jittered <= plain + 1_800_000, "jitter capped at 30 min");
    }

    #[test]
    fn jitter_zero_is_no_op() {
        let plain = compute_next_run("0 * * * *").unwrap().unwrap();
        let jittered = compute_next_run_jittered("0 * * * *", Some(0)).unwrap().unwrap();
        assert_eq!(plain, jittered);
    }

    #[test]
    fn jitter_explicit_max_respected() {
        let plain = compute_next_run("0 * * * *").unwrap().unwrap();
        for _ in 0..10 {
            let j = compute_next_run_jittered("0 * * * *", Some(5_000)).unwrap().unwrap();
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
        assert_eq!(to_6field("30 */5 * * * *"), "30 */5 * * * *");
    }

    #[test]
    fn test_invalid_schedule() {
        assert!(validate_schedule("not a cron").is_err());
        assert!(validate_schedule("* * *").is_err());
        assert!(validate_schedule("0 9 * * *").is_ok());
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
        // "0 3 * * *" in Europe/Riga (UTC+2 winter / +3 summer) fires at
        // 00:00-01:00 UTC depending on DST. UTC interpretation fires at 03:00 UTC.
        // The two MUST differ by 2-3 hours, never zero.
        let utc = compute_next_run_in("0 3 * * *", None).unwrap().unwrap();
        let riga = compute_next_run_in("0 3 * * *", Some("Europe/Riga")).unwrap().unwrap();
        let diff_ms = (utc as i64 - riga as i64).abs();
        // Could be the same calendar day or different — both legitimate.
        // The shift between them mod 24h is 2-3 hours.
        let day_ms: i64 = 24 * 60 * 60 * 1000;
        let mod_diff = diff_ms.rem_euclid(day_ms);
        assert!(
            mod_diff == 2 * 3_600_000 || mod_diff == 3 * 3_600_000
              || (day_ms - mod_diff) == 2 * 3_600_000 || (day_ms - mod_diff) == 3 * 3_600_000,
            "expected ±2 or ±3h shift between UTC and Europe/Riga, got mod_diff={}", mod_diff
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
        let utc = compute_next_run_jittered_in("0 3 * * *", Some(0), None).unwrap().unwrap();
        let riga = compute_next_run_jittered_in("0 3 * * *", Some(0), Some("Europe/Riga")).unwrap().unwrap();
        // Jitter=0 → deterministic; tz shift must show up.
        assert_ne!(utc, riga);
    }

    #[test]
    fn tz_invalid_via_jittered_errors() {
        assert!(compute_next_run_jittered_in("0 3 * * *", None, Some("Mars/Time")).is_err());
    }
}
