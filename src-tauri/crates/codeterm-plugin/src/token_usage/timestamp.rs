use serde_json::Value;

pub(super) fn record_timestamp(value: &Value, now: u64) -> u64 {
    if let Some(secs) = value.get("timestamp").and_then(rfc3339_secs) {
        return secs;
    }
    if let Some(ms) = value
        .get("time")
        .and_then(|t| t.get("created"))
        .and_then(Value::as_u64)
    {
        return ms / 1000;
    }
    now
}

/// Minimal RFC3339 → unix seconds. Provider logs stamp UTC `Z` timestamps; anything
/// else falls through to the ingest clock rather than guessing an offset.
fn rfc3339_secs(value: &Value) -> Option<u64> {
    if let Some(secs) = value.as_u64() {
        return Some(secs);
    }
    let (date, rest) = value.as_str()?.split_once('T')?;
    let time = rest.strip_suffix('Z')?;
    let mut date_parts = date.split('-');
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month: i64 = date_parts.next()?.parse().ok()?;
    let day: i64 = date_parts.next()?.parse().ok()?;
    let mut time_parts = time.split(':');
    let hour: i64 = time_parts.next()?.parse().ok()?;
    let minute: i64 = time_parts.next()?.parse().ok()?;
    let second: i64 = time_parts.next()?.split('.').next()?.parse::<f64>().ok()? as i64;
    let days = days_from_civil(year, month, day);
    let secs = days * 86_400 + hour * 3_600 + minute * 60 + second;
    u64::try_from(secs).ok()
}

/// Howard Hinnant's civil-from-days inverse; avoids a chrono dependency for one field.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}
