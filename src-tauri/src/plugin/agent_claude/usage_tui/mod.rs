mod resets;
mod session_block;

#[cfg(test)]
mod tests;

use chrono::Utc;

use crate::agent::usage_parser::{UsageParseError, UsageParser, UsageSnapshot};

pub struct ClaudeTuiUsageParser;

impl ClaudeTuiUsageParser {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ClaudeTuiUsageParser {
    fn default() -> Self {
        Self::new()
    }
}

impl UsageParser for ClaudeTuiUsageParser {
    fn parse(&self, raw_text: &str) -> Result<UsageSnapshot, UsageParseError> {
        if raw_text.trim().is_empty() {
            return Err(UsageParseError::NothingFound);
        }

        let now_ms = Utc::now().timestamp_millis();
        let normalized = normalize(raw_text);
        let mut snap = UsageSnapshot::empty("claude", raw_text, now_ms);

        let (s_pct, s_resets_ms) = session_block::parse_current_session(&normalized, now_ms);
        snap.session_pct = s_pct;
        snap.session_resets_at_ms = s_resets_ms;

        let (w_pct, w_resets_ms) = session_block::parse_current_week_all(&normalized, now_ms);
        snap.weekly_pct = w_pct;
        snap.weekly_resets_at_ms = w_resets_ms;

        if !snap.has_any_field() {
            return Err(UsageParseError::NothingFound);
        }
        Ok(snap)
    }
}

fn normalize(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch == '\u{1b}' || ch == '\r' {
            continue;
        }
        if ch == '█' || ch == '▌' || ch == '▍' || ch == '▎' || ch == '▏' || ch == '▊' || ch == '▋' {
            out.push(' ');
            continue;
        }
        out.push(ch);
    }
    out
}
