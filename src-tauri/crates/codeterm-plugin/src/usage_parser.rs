use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSnapshot {
    pub provider: String,
    #[serde(default)]
    pub billing_mode: codeterm_usage::contract::BillingMode,
    #[serde(default)]
    pub limits_reported: bool,
    pub account_id: Option<String>,
    pub captured_at_ms: i64,
    pub session_pct: Option<f32>,
    pub session_resets_at_ms: Option<i64>,
    pub weekly_pct: Option<f32>,
    pub weekly_resets_at_ms: Option<i64>,
    /// Spend-based providers (e.g. cursor): cents used in the current billing
    /// cycle. None for percentage-based providers.
    #[serde(default)]
    pub spend_cents: Option<f64>,
    #[serde(default)]
    pub spend_limit_cents: Option<f64>,
    #[serde(default)]
    pub spend_remaining_cents: Option<f64>,
    #[serde(default)]
    pub spend_resets_at_ms: Option<i64>,
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

#[derive(Debug, Clone)]
pub enum UsageParseOutcome {
    Parsed(UsageSnapshot),
    Skip(String),
}

pub trait UsageParser: Send + Sync {
    fn parse(&self, raw_text: &str) -> Result<UsageSnapshot, UsageParseError>;

    fn parse_outcome(&self, raw_text: &str) -> Result<UsageParseOutcome, UsageParseError> {
        match self.parse(raw_text) {
            Ok(snapshot) => Ok(UsageParseOutcome::Parsed(snapshot)),
            Err(UsageParseError::NothingFound) => Ok(UsageParseOutcome::Skip(
                "nothing recognizable found in input".to_string(),
            )),
            Err(e) => Err(e),
        }
    }
}

impl UsageSnapshot {
    pub fn empty(provider: &str, raw_text: &str, captured_at_ms: i64) -> Self {
        Self {
            provider: provider.to_string(),
            billing_mode: Default::default(),
            limits_reported: false,
            account_id: None,
            captured_at_ms,
            session_pct: None,
            session_resets_at_ms: None,
            weekly_pct: None,
            weekly_resets_at_ms: None,
            spend_cents: None,
            spend_limit_cents: None,
            spend_remaining_cents: None,
            spend_resets_at_ms: None,
            raw_text: raw_text.to_string(),
        }
    }

    pub fn has_any_field(&self) -> bool {
        self.session_pct.is_some()
            || self.weekly_pct.is_some()
            || self.spend_cents.is_some()
            || self.spend_limit_cents.is_some()
            || self.spend_remaining_cents.is_some()
    }
}

#[cfg(test)]
mod billing_tests {
    use super::*;

    #[test]
    fn billing_metadata_keeps_missing_limits_and_spend_unknown() {
        let mut value = serde_json::to_value(UsageSnapshot::empty("fixture", "{}", 1000)).unwrap();
        value["billing_mode"] = serde_json::json!("subscription");
        let snapshot: UsageSnapshot = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(
            snapshot.billing_mode,
            codeterm_usage::contract::BillingMode::Subscription
        );
        assert!(!snapshot.limits_reported);
        assert_eq!(snapshot.spend_cents, None);
        value.as_object_mut().unwrap().remove("billing_mode");
        value.as_object_mut().unwrap().remove("limits_reported");
        let snapshot: UsageSnapshot = serde_json::from_value(value).unwrap();
        assert_eq!(
            snapshot.billing_mode,
            codeterm_usage::contract::BillingMode::Unknown
        );
        assert!(!snapshot.limits_reported);
    }
}
