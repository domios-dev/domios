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
    let marker_re = match Regex::new(marker) {
        Ok(re) => re,
        Err(_) => return (None, None),
    };
    let marker_match = match marker_re.find(text) {
        Some(found) => found,
        None => return (None, None),
    };
    let tail = &text[marker_match.end()..];
    let limit = tail
        .char_indices()
        .nth(800)
        .map(|(index, _)| index)
        .unwrap_or(tail.len());
    let limited = &tail[..limit];
    let next_block = Regex::new(r"(?im)^\s*Current\s+")
        .ok()
        .and_then(|re| re.find(limited))
        .map(|found| found.start())
        .unwrap_or(limited.len());
    let block = &limited[..next_block];
    let pct_re = match Regex::new(r"(\d+(?:\.\d+)?)\s*%\s*used") {
        Ok(re) => re,
        Err(_) => return (None, None),
    };
    let pct_caps = match pct_re.captures(block) {
        Some(c) => c,
        None => return (None, None),
    };
    let pct = pct_caps.get(1).and_then(|m| m.as_str().parse::<f32>().ok());
    let reset_re = match Regex::new(r"Resets?\s*([^()\n]+?)\s*\(") {
        Ok(re) => re,
        Err(_) => return (pct, None),
    };
    let reset_caps = reset_re.captures(block);
    let time_raw = reset_caps
        .as_ref()
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().trim())
        .unwrap_or("");
    let resets_ms = if is_weekly {
        resets::parse_weekly_reset(time_raw, now_ms)
    } else {
        resets::parse_today_clock(time_raw, now_ms)
    };
    (pct, resets_ms)
}
