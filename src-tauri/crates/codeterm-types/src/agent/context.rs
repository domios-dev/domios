use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnContext {
    pub used_tokens: u64,
    pub window_tokens: u64,
    pub cost_usd: Option<f64>,
    pub source: String,
}

impl TurnContext {
    pub fn valid(&self) -> bool {
        self.used_tokens <= 10_000_000
            && self.window_tokens <= 10_000_000
            && self
                .cost_usd
                .is_none_or(|cost| cost.is_finite() && (0.0..=1_000_000.0).contains(&cost))
            && !self.source.is_empty()
            && self.source.len() <= 64
            && self
                .source
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    }
}
