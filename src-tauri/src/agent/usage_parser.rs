use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSnapshot {
    pub provider: String,
    pub account_id: Option<String>,
    pub captured_at_ms: i64,
    pub session_pct: Option<f32>,
    pub session_resets_at_ms: Option<i64>,
    pub weekly_pct: Option<f32>,
    pub weekly_resets_at_ms: Option<i64>,
    pub weekly_sonnet_pct: Option<f32>,
    pub weekly_sonnet_resets_at_ms: Option<i64>,
    pub session_cost_usd: Option<f32>,
    pub session_api_duration_s: Option<i64>,
    pub session_wall_duration_s: Option<i64>,
    pub session_lines_added: Option<i64>,
    pub session_lines_removed: Option<i64>,
    pub session_tokens_input: Option<i64>,
    pub session_tokens_output: Option<i64>,
    pub session_tokens_cache_read: Option<i64>,
    pub session_tokens_cache_write: Option<i64>,
    pub breakdown: Vec<UsageBreakdownRow>,
    pub raw_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageBreakdownRow {
    pub dimension: BreakdownDimension,
    pub key: String,
    pub pct: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum BreakdownDimension {
    Skill,
    Subagent,
    Plugin,
    Heuristic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageParseError {
    NothingFound,
    Other(String),
}

impl std::fmt::Display for UsageParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NothingFound => write!(f, "nothing recognizable found in input"),
            Self::Other(s) => write!(f, "parse failed: {}", s),
        }
    }
}

impl std::error::Error for UsageParseError {}

pub trait UsageParser: Send + Sync {
    fn parse(&self, raw_text: &str) -> Result<UsageSnapshot, UsageParseError>;
}

impl UsageSnapshot {
    pub fn empty(provider: &str, raw_text: &str, captured_at_ms: i64) -> Self {
        Self {
            provider: provider.to_string(),
            account_id: None,
            captured_at_ms,
            session_pct: None,
            session_resets_at_ms: None,
            weekly_pct: None,
            weekly_resets_at_ms: None,
            weekly_sonnet_pct: None,
            weekly_sonnet_resets_at_ms: None,
            session_cost_usd: None,
            session_api_duration_s: None,
            session_wall_duration_s: None,
            session_lines_added: None,
            session_lines_removed: None,
            session_tokens_input: None,
            session_tokens_output: None,
            session_tokens_cache_read: None,
            session_tokens_cache_write: None,
            breakdown: Vec::new(),
            raw_text: raw_text.to_string(),
        }
    }

    pub fn has_any_field(&self) -> bool {
        self.session_pct.is_some()
            || self.weekly_pct.is_some()
            || self.weekly_sonnet_pct.is_some()
            || self.session_cost_usd.is_some()
            || self.session_api_duration_s.is_some()
            || self.session_wall_duration_s.is_some()
            || self.session_lines_added.is_some()
            || self.session_lines_removed.is_some()
            || self.session_tokens_input.is_some()
            || self.session_tokens_output.is_some()
            || self.session_tokens_cache_read.is_some()
            || self.session_tokens_cache_write.is_some()
            || !self.breakdown.is_empty()
    }
}
