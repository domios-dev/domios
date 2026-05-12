use chrono::{Datelike, Local, NaiveDateTime, TimeZone, Timelike, Utc};
use regex::Regex;

use crate::agent::usage_parser::{UsageParseError, UsageParser, UsageSnapshot};

pub struct CodexStatusUsageParser;

impl CodexStatusUsageParser {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CodexStatusUsageParser {
    fn default() -> Self {
        Self::new()
    }
}

impl UsageParser for CodexStatusUsageParser {
    fn parse(&self, raw_text: &str) -> Result<UsageSnapshot, UsageParseError> {
        if raw_text.trim().is_empty() {
            return Err(UsageParseError::NothingFound);
        }
        let now_ms = Utc::now().timestamp_millis();
        let normalized = normalize(raw_text);
        let mut snap = UsageSnapshot::empty("codex", raw_text, now_ms);

        let (s_pct, s_resets_ms) = parse_gauge(&normalized, r"5h\s*limit", false, now_ms);
        snap.session_pct = s_pct;
        snap.session_resets_at_ms = s_resets_ms;

        let (w_pct, w_resets_ms) = parse_gauge(&normalized, r"Weekly\s*limit", true, now_ms);
        snap.weekly_pct = w_pct;
        snap.weekly_resets_at_ms = w_resets_ms;

        if !snap.has_any_field() {
            return Err(UsageParseError::NothingFound);
        }
        Ok(snap)
    }
}

fn normalize(raw: &str) -> String {
    raw.replace('\r', "").replace('█', " ").replace('░', " ")
}

fn parse_gauge(text: &str, marker: &str, is_weekly: bool, now_ms: i64) -> (Option<f32>, Option<i64>) {
    let pattern = format!(
        r"{marker}[\s\S]{{0,200}}?(\d+(?:\.\d+)?)\s*%\s*left[\s\S]*?\(resets?\s+([^)]+?)\)",
        marker = marker
    );
    let re = match Regex::new(&pattern) {
        Ok(re) => re,
        Err(_) => return (None, None),
    };
    let caps = match re.captures(text) {
        Some(c) => c,
        None => return (None, None),
    };
    let pct_left = caps
        .get(1)
        .and_then(|m| m.as_str().parse::<f32>().ok())
        .map(|left| (100.0 - left).clamp(0.0, 100.0));
    let resets_raw = caps.get(2).map(|m| m.as_str().trim()).unwrap_or("");
    let resets_ms = if is_weekly {
        parse_weekly_resets(resets_raw, now_ms)
    } else {
        parse_session_resets(resets_raw, now_ms)
    };
    (pct_left, resets_ms)
}

fn parse_session_resets(raw: &str, now_ms: i64) -> Option<i64> {
    let re = Regex::new(r"(\d{1,2}):(\d{2})").ok()?;
    let caps = re.captures(raw.trim())?;
    let hh: u32 = caps.get(1)?.as_str().parse().ok()?;
    let mm: u32 = caps.get(2)?.as_str().parse().ok()?;
    let now_dt = Local.timestamp_millis_opt(now_ms).single()?;
    let mut target = now_dt.with_hour(hh)?.with_minute(mm)?.with_second(0)?.with_nanosecond(0)?;
    if target <= now_dt {
        target += chrono::Duration::days(1);
    }
    Some(target.timestamp_millis())
}

fn parse_weekly_resets(raw: &str, now_ms: i64) -> Option<i64> {
    let re = Regex::new(r"(\d{1,2}):(\d{2})\s+on\s+(\d{1,2})\s+([A-Za-z]+)").ok()?;
    let caps = re.captures(raw.trim())?;
    let hh: u32 = caps.get(1)?.as_str().parse().ok()?;
    let mm: u32 = caps.get(2)?.as_str().parse().ok()?;
    let day: u32 = caps.get(3)?.as_str().parse().ok()?;
    let month = month_from_short(caps.get(4)?.as_str())?;
    let now_dt = Local.timestamp_millis_opt(now_ms).single()?;
    let mut year = now_dt.year();
    let naive = NaiveDateTime::new(
        chrono::NaiveDate::from_ymd_opt(year, month, day)?,
        chrono::NaiveTime::from_hms_opt(hh, mm, 0)?,
    );
    let mut dt = Local.from_local_datetime(&naive).single()?;
    if dt <= now_dt {
        year += 1;
        let naive_next = NaiveDateTime::new(
            chrono::NaiveDate::from_ymd_opt(year, month, day)?,
            chrono::NaiveTime::from_hms_opt(hh, mm, 0)?,
        );
        dt = Local.from_local_datetime(&naive_next).single()?;
    }
    Some(dt.timestamp_millis())
}

fn month_from_short(name: &str) -> Option<u32> {
    match name.to_ascii_lowercase().as_str() {
        "jan" | "january" => Some(1),
        "feb" | "february" => Some(2),
        "mar" | "march" => Some(3),
        "apr" | "april" => Some(4),
        "may" => Some(5),
        "jun" | "june" => Some(6),
        "jul" | "july" => Some(7),
        "aug" | "august" => Some(8),
        "sep" | "sept" | "september" => Some(9),
        "oct" | "october" => Some(10),
        "nov" | "november" => Some(11),
        "dec" | "december" => Some(12),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("fixtures/status_v0_130.txt");

    #[test]
    fn parses_fixture_v0_130() {
        let p = CodexStatusUsageParser::new();
        let snap = p.parse(FIXTURE).expect("parse");
        assert_eq!(snap.session_pct, Some(10.0), "90% left -> 10% used");
        assert_eq!(snap.weekly_pct, Some(80.0), "20% left -> 80% used");
        assert!(snap.session_resets_at_ms.is_some());
        assert!(snap.weekly_resets_at_ms.is_some());
    }

    #[test]
    fn empty_input_errors() {
        let p = CodexStatusUsageParser::new();
        assert!(matches!(p.parse(""), Err(UsageParseError::NothingFound)));
    }

    #[test]
    fn garbled_returns_err() {
        let p = CodexStatusUsageParser::new();
        assert!(matches!(p.parse("no markers here"), Err(UsageParseError::NothingFound)));
    }
}
