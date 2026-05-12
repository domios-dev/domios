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
    pub raw_text: String,
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
            raw_text: raw_text.to_string(),
        }
    }

    pub fn has_any_field(&self) -> bool {
        self.session_pct.is_some() || self.weekly_pct.is_some()
    }
}
