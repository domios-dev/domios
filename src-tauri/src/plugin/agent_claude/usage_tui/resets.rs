use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone};

pub fn parse_today_clock(s: &str, now_ms: i64) -> Option<i64> {
    let (hh, mm, pm) = parse_h_mm_ampm(s)?;
    let now = Local.timestamp_millis_opt(now_ms).single()?;
    let mut hour24 = hh % 12;
    if pm {
        hour24 += 12;
    }
    let candidate_naive = now.date_naive().and_hms_opt(hour24, mm, 0)?;
    let candidate = Local.from_local_datetime(&candidate_naive).single()?;
    let candidate = if candidate <= now {
        candidate + Duration::days(1)
    } else {
        candidate
    };
    Some(candidate.timestamp_millis())
}

pub fn parse_month_day_clock(s: &str, now_ms: i64) -> Option<i64> {
    let trimmed = s.trim();
    let lower = trimmed.to_ascii_lowercase();
    let parts: Vec<&str> = lower.split_whitespace().collect();
    if parts.len() < 4 {
        return None;
    }
    let month = month_from_name(parts[0])?;
    let day: u32 = parts[1].parse().ok()?;
    if parts[2] != "at" {
        return None;
    }
    let (hh, mm, pm) = parse_h_mm_ampm(&parts[3..].join(" "))?;
    let now = Local.timestamp_millis_opt(now_ms).single()?;
    let mut hour24 = hh % 12;
    if pm {
        hour24 += 12;
    }
    let year = now.year();
    let candidate = build_local(year, month, day, hour24, mm)
        .or_else(|| build_local(year + 1, month, day, hour24, mm))?;
    let candidate = if candidate < now {
        build_local(year + 1, month, day, hour24, mm).unwrap_or(candidate)
    } else {
        candidate
    };
    Some(candidate.timestamp_millis())
}

fn build_local(year: i32, month: u32, day: u32, hh: u32, mm: u32) -> Option<DateTime<Local>> {
    let nd = NaiveDate::from_ymd_opt(year, month, day)?;
    let ndt = nd.and_hms_opt(hh, mm, 0)?;
    Local.from_local_datetime(&ndt).single()
}

fn month_from_name(s: &str) -> Option<u32> {
    Some(match s {
        "jan" | "january" => 1,
        "feb" | "february" => 2,
        "mar" | "march" => 3,
        "apr" | "april" => 4,
        "may" => 5,
        "jun" | "june" => 6,
        "jul" | "july" => 7,
        "aug" | "august" => 8,
        "sep" | "sept" | "september" => 9,
        "oct" | "october" => 10,
        "nov" | "november" => 11,
        "dec" | "december" => 12,
        _ => return None,
    })
}

fn parse_h_mm_ampm(s: &str) -> Option<(u32, u32, bool)> {
    let s = s.trim().to_ascii_lowercase();
    let pm = if s.ends_with("pm") {
        true
    } else if s.ends_with("am") {
        false
    } else {
        return None;
    };
    let stripped = &s[..s.len() - 2];
    let stripped = stripped.trim();
    let (h_part, m_part) = match stripped.split_once(':') {
        Some((h, m)) => (h, m),
        None => (stripped, "0"),
    };
    let hh: u32 = h_part.trim().parse().ok()?;
    let mm: u32 = m_part.trim().parse().ok()?;
    if hh == 0 || hh > 12 || mm > 59 {
        return None;
    }
    Some((hh, mm, pm))
}
