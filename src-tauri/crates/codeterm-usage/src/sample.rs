#[derive(Debug, Clone)]
pub struct UsageSample {
    pub provider: String,
    pub billing_mode: codeterm_types::usage::BillingMode,
    pub limits_reported: bool,
    pub account_id: Option<String>,
    pub captured_at_ms: i64,
    pub session_pct: Option<f32>,
    pub session_resets_at_ms: Option<i64>,
    pub weekly_pct: Option<f32>,
    pub weekly_resets_at_ms: Option<i64>,
    pub spend_cents: Option<f64>,
    pub spend_limit_cents: Option<f64>,
    pub spend_remaining_cents: Option<f64>,
    pub spend_resets_at_ms: Option<i64>,
    pub raw_text: String,
}

#[cfg(test)]
impl UsageSample {
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
}
