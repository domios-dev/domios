mod resets;
mod session_block;
mod totals;
mod breakdown;

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

        let (ws_pct, ws_resets_ms) = session_block::parse_current_week_sonnet(&normalized, now_ms);
        snap.weekly_sonnet_pct = ws_pct;
        snap.weekly_sonnet_resets_at_ms = ws_resets_ms;

        let totals = totals::parse_totals(&normalized);
        snap.session_cost_usd = totals.cost_usd;
        snap.session_api_duration_s = totals.api_duration_s;
        snap.session_wall_duration_s = totals.wall_duration_s;
        snap.session_lines_added = totals.lines_added;
        snap.session_lines_removed = totals.lines_removed;
        snap.session_tokens_input = totals.tokens_input;
        snap.session_tokens_output = totals.tokens_output;
        snap.session_tokens_cache_read = totals.tokens_cache_read;
        snap.session_tokens_cache_write = totals.tokens_cache_write;

        snap.breakdown = breakdown::parse_all(raw_text, &normalized);

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
