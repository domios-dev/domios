//! Schedule parsing, validation, and next-fire-time computation.
//!
//! Pure functions — no state, no I/O. Used by both [`super::CronStore`] and
//! external callers (cron::scheduler tests, etc.) via the parent's `pub use`.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::occurrence::Occurrence;

pub const MAX_CONSECUTIVE_FAILURES: u32 = 5;
const MAX_SCHEDULE_LOOKAHEAD_MS: u64 = 5 * 366 * 24 * 60 * 60 * 1000;
const MAX_AUTO_JITTER_MS: u64 = 30 * 60 * 1000;

fn system_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// First two fire times strictly after `now_ms`, in the given tz
/// (None → UTC). The injectable-now core: `Schedule::upcoming` hardcodes
/// the real current time, which made cron untestable without wall-clock
/// waits — `Schedule::after` takes our time instead.
fn first_two_fires_after(
    sched: &cron::Schedule,
    tz: Option<chrono_tz::Tz>,
    now_ms: u64,
) -> (Option<u64>, Option<u64>) {
    use chrono::TimeZone;
    match tz {
        Some(tz) => {
            let now = match tz.timestamp_millis_opt(now_ms as i64).single() {
                Some(dt) => dt,
                None => return (None, None),
            };
            let mut it = sched.after(&now).map(|dt| dt.timestamp_millis() as u64);
            (it.next(), it.next())
        }
        None => {
            let now = match chrono::Utc.timestamp_millis_opt(now_ms as i64).single() {
                Some(dt) => dt,
                None => return (None, None),
            };
            let mut it = sched.after(&now).map(|dt| dt.timestamp_millis() as u64);
            (it.next(), it.next())
        }
    }
}

fn parse_schedule(schedule: &str) -> Result<cron::Schedule, String> {
    use std::str::FromStr;
    if schedule.trim().starts_with("in ") {
        return Err("schedule: relative times require oneshot=true".into());
    }
    let fields: Vec<&str> = schedule.split_whitespace().collect();
    if fields.len() != 5 {
        return Err(format!(
            "schedule: expected 5 fields (minute hour day-of-month month day-of-week), got {}",
            fields.len()
        ));
    }
    cron::Schedule::from_str(&to_6field(schedule))
        .map_err(|error| format!("schedule: invalid 5-field cron expression: {error}"))
}

fn upcoming_fires(
    schedule: &str,
    tz: Option<&str>,
    now_ms: u64,
) -> Result<(u64, Option<u64>), String> {
    let sched = parse_schedule(schedule)?;
    let parsed_tz = parse_tz(tz)?;
    let (next, following) = first_two_fires_after(&sched, parsed_tz, now_ms);
    let horizon = now_ms.saturating_add(MAX_SCHEDULE_LOOKAHEAD_MS);
    let next = next
        .filter(|fire| *fire <= horizon)
        .ok_or_else(|| "schedule: no fire time within the next 5 years".to_string())?;
    Ok((next, following))
}

pub fn cap_jitter_max_ms_at(
    schedule: &str,
    jitter_max_ms: Option<u64>,
    tz: Option<&str>,
    now_ms: u64,
) -> Result<Option<u64>, String> {
    let Some(requested) = jitter_max_ms else {
        return Ok(None);
    };
    let (next, following) = upcoming_fires(schedule, tz, now_ms)?;
    let cap = following
        .map(|fire| fire.saturating_sub(next) / 2)
        .unwrap_or_default();
    Ok(Some(requested.min(cap)))
}

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
            .map_err(|e| format!("tz: invalid timezone '{}': {}", s, e)),
    }
}

pub fn compute_next_run(schedule: &str) -> Result<Option<u64>, String> {
    compute_next_run_in(schedule, None)
}

pub fn compute_next_run_in(schedule: &str, tz: Option<&str>) -> Result<Option<u64>, String> {
    compute_next_run_at(schedule, tz, system_now_ms())
}

pub fn compute_next_run_at(
    schedule: &str,
    tz: Option<&str>,
    now_ms: u64,
) -> Result<Option<u64>, String> {
    upcoming_fires(schedule, tz, now_ms).map(|(next, _)| Some(next))
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
    compute_next_run_jittered_at(schedule, jitter_max_ms, tz, system_now_ms())
}

pub fn compute_next_run_jittered_at(
    schedule: &str,
    jitter_max_ms: Option<u64>,
    tz: Option<&str>,
    now_ms: u64,
) -> Result<Option<u64>, String> {
    Ok(
        next_occurrence_at(schedule, jitter_max_ms, tz, now_ms)?
            .map(|occurrence| occurrence.due_at),
    )
}

pub fn next_occurrence_at(
    schedule: &str,
    jitter_max_ms: Option<u64>,
    tz: Option<&str>,
    now_ms: u64,
) -> Result<Option<Occurrence>, String> {
    let (next, second) = upcoming_fires(schedule, tz, now_ms)?;
    let max = jitter_limit(jitter_max_ms, next, second);
    use rand::Rng;
    let offset = if max == 0 {
        0
    } else {
        rand::thread_rng().gen_range(0..=max)
    };
    Ok(Some(Occurrence {
        scheduled_at: next,
        due_at: next.saturating_add(offset),
    }))
}

pub fn restore_occurrence_at(
    schedule: &str,
    jitter_max_ms: Option<u64>,
    tz: Option<&str>,
    due_at: u64,
) -> Result<Option<Occurrence>, String> {
    use chrono::TimeZone;

    fn previous<Z: TimeZone>(schedule: &cron::Schedule, zone: Z, before: i64) -> Option<u64> {
        let before = zone.timestamp_millis_opt(before).single()?;
        schedule
            .after(&before)
            .next_back()
            .and_then(|previous| previous.timestamp_millis().try_into().ok())
    }

    let schedule = parse_schedule(schedule)?;
    let tz = parse_tz(tz)?;
    let Ok(before) = i64::try_from(due_at.saturating_add(1)) else {
        return Ok(None);
    };
    let scheduled_at = match tz {
        Some(zone) => previous(&schedule, zone, before),
        None => previous(&schedule, chrono::Utc, before),
    };
    let Some(scheduled_at) = scheduled_at.filter(|at| *at <= due_at) else {
        return Ok(None);
    };
    let (following, _) = first_two_fires_after(&schedule, tz, scheduled_at);
    Ok(
        (due_at - scheduled_at <= jitter_limit(jitter_max_ms, scheduled_at, following)).then_some(
            Occurrence {
                scheduled_at,
                due_at,
            },
        ),
    )
}

fn jitter_limit(jitter_max_ms: Option<u64>, occurrence: u64, following: Option<u64>) -> u64 {
    let interval = following.map_or(0, |following| following.saturating_sub(occurrence));
    match jitter_max_ms {
        Some(requested) => requested.min(interval / 2),
        None => (interval / 10).min(MAX_AUTO_JITTER_MS),
    }
}

pub fn validate_schedule(schedule: &str) -> Result<(), String> {
    upcoming_fires(schedule, None, system_now_ms()).map(|_| ())
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
    parse_oneshot_schedule_at(schedule, system_now_ms())
}

pub fn parse_oneshot_schedule_at(schedule: &str, now_ms: u64) -> Result<u64, String> {
    let s = schedule.trim();

    if let Some(rest) = s.strip_prefix("in ") {
        let duration_ms = parse_relative_duration(rest.trim())?;
        if duration_ms == 0 {
            return Err("Duration must be > 0".into());
        }
        return Ok(now_ms + duration_ms);
    }

    if s.len() >= 13 && s.chars().all(|c| c.is_ascii_digit()) {
        let ts: u64 = s.parse().map_err(|_| "Invalid timestamp".to_string())?;
        if ts <= now_ms {
            return Err("Oneshot time must be in the future".into());
        }
        return Ok(ts);
    }

    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        let ts = dt.timestamp_millis() as u64;
        if ts <= now_ms {
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
            let n: u64 = num_buf
                .parse()
                .map_err(|_| format!("Invalid number in duration: '{}'", s))?;
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
        let n: u64 = num_buf
            .parse()
            .map_err(|_| format!("Invalid number in duration: '{}'", s))?;
        total_ms += n * 60_000;
    }

    if total_ms == 0 {
        return Err(format!("Could not parse duration from '{}'", s));
    }

    Ok(total_ms)
}

#[cfg(test)]
mod tests;
