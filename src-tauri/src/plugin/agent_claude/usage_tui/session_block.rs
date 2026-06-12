use regex::Regex;

use super::resets;

pub fn parse_current_session(text: &str, now_ms: i64) -> (Option<f32>, Option<i64>) {
    parse_block(text, r"Current session", false, now_ms)
}

pub fn parse_current_week_all(text: &str, now_ms: i64) -> (Option<f32>, Option<i64>) {
    parse_block(text, r"Current week\s*\(\s*all\s*models\s*\)", true, now_ms)
}

fn parse_block(
    text: &str,
    marker: &str,
    is_weekly: bool,
    now_ms: i64,
) -> (Option<f32>, Option<i64>) {
    let pattern = format!(
        r"{marker}[\s\S]{{0,800}}?(\d+(?:\.\d+)?)\s*%\s*used[\s\S]*?Resets?\s*([^()\n]+?)\s*\(",
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
    let pct = caps.get(1).and_then(|m| m.as_str().parse::<f32>().ok());
    let time_raw = caps.get(2).map(|m| m.as_str().trim()).unwrap_or("");
    let resets_ms = if is_weekly {
        resets::parse_month_day_clock(time_raw, now_ms)
    } else {
        resets::parse_today_clock(time_raw, now_ms)
    };
    (pct, resets_ms)
}
