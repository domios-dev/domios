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

pub fn parse_weekly_reset(s: &str, now_ms: i64) -> Option<i64> {
    parse_month_day_clock(s, now_ms)
        .or_else(|| parse_relative_day_clock(s, now_ms))
        .or_else(|| parse_today_clock(s, now_ms))
}

pub fn parse_month_day_clock(s: &str, now_ms: i64) -> Option<i64> {
    let mut parts = cleaned_parts(s);
    while parts
        .first()
        .map(|p| is_weekday(p.as_str()))
        .unwrap_or(false)
    {
        parts.remove(0);
    }
    if parts.len() < 3 {
        return None;
    }
    let month = month_from_name(&parts[0])?;
    let day = parse_day_token(&parts[1])?;
    let mut idx = 2;
    let explicit_year = parts.get(idx).and_then(|p| p.parse::<i32>().ok());
    if explicit_year.is_some() {
        idx += 1;
    }
    if parts.get(idx).map(|p| p.as_str()) == Some("at") {
        idx += 1;
    }
    let (hh, mm, pm) = parse_h_mm_ampm(&parts[idx..].join(" "))?;
    let now = Local.timestamp_millis_opt(now_ms).single()?;
    let mut hour24 = hh % 12;
    if pm {
        hour24 += 12;
    }
    let year = explicit_year.unwrap_or_else(|| now.year());
    let candidate = build_local(year, month, day, hour24, mm)
        .or_else(|| build_local(year + 1, month, day, hour24, mm))?;
    let candidate = if explicit_year.is_none() && candidate < now {
        build_local(year + 1, month, day, hour24, mm).unwrap_or(candidate)
    } else {
        candidate
    };
    Some(candidate.timestamp_millis())
}

fn parse_relative_day_clock(s: &str, now_ms: i64) -> Option<i64> {
    let parts = cleaned_parts(s);
    let (offset_days, idx) = match parts.first().map(|p| p.as_str()) {
        Some("today") | Some("tonight") => (0, 1),
        Some("tomorrow") => (1, 1),
        _ => return None,
    };
    let idx = if parts.get(idx).map(|p| p.as_str()) == Some("at") {
        idx + 1
    } else {
        idx
    };
    let (hh, mm, pm) = parse_h_mm_ampm(&parts[idx..].join(" "))?;
    let now = Local.timestamp_millis_opt(now_ms).single()?;
    let mut hour24 = hh % 12;
    if pm {
        hour24 += 12;
    }
    let candidate_naive = now
        .date_naive()
        .checked_add_signed(Duration::days(offset_days))?
        .and_hms_opt(hour24, mm, 0)?;
    let candidate = Local.from_local_datetime(&candidate_naive).single()?;
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

fn cleaned_parts(s: &str) -> Vec<String> {
    s.to_ascii_lowercase()
        .replace(',', " ")
        .split_whitespace()
        .map(|p| p.trim_matches(|c: char| c == '.' || c == ';').to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

fn parse_day_token(s: &str) -> Option<u32> {
    let day = s
        .trim_end_matches("st")
        .trim_end_matches("nd")
        .trim_end_matches("rd")
        .trim_end_matches("th")
        .parse()
        .ok()?;
    (1..=31).contains(&day).then_some(day)
}

fn is_weekday(s: &str) -> bool {
    matches!(
        s,
        "mon"
            | "monday"
            | "tue"
            | "tues"
            | "tuesday"
            | "wed"
            | "wednesday"
            | "thu"
            | "thur"
            | "thurs"
            | "thursday"
            | "fri"
            | "friday"
            | "sat"
            | "saturday"
            | "sun"
            | "sunday"
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> i64 {
        Local
            .with_ymd_and_hms(year, month, day, hour, minute, 0)
            .single()
            .unwrap()
            .timestamp_millis()
    }

    #[test]
    fn weekly_reset_parses_explicit_year_and_comma() {
        let now = ms(2026, 6, 14, 15, 0);
        let got = parse_weekly_reset("Jun 14, 2026 at 4 PM", now).unwrap();
        assert_eq!(got, ms(2026, 6, 14, 16, 0));
    }

    #[test]
    fn weekly_reset_parses_bare_clock() {
        let now = ms(2026, 6, 14, 15, 0);
        let got = parse_weekly_reset("4 PM", now).unwrap();
        assert_eq!(got, ms(2026, 6, 14, 16, 0));
    }

    #[test]
    fn weekly_reset_parses_relative_day() {
        let now = ms(2026, 6, 14, 15, 0);
        let got = parse_weekly_reset("tomorrow at 9:30 AM", now).unwrap();
        assert_eq!(got, ms(2026, 6, 15, 9, 30));
    }

    #[test]
    fn weekly_reset_parses_month_day_comma_time() {
        let now = ms(2026, 6, 14, 15, 0);
        let got = parse_weekly_reset("Jun 14, 7:59pm", now).unwrap();
        assert_eq!(got, ms(2026, 6, 14, 19, 59));
    }
}
