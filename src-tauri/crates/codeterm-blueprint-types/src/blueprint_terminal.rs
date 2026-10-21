use serde::{Deserialize, Serialize};

pub const SESSION_SAFETY_LIMIT: usize = 16_384;
pub const SESSION_RATE_LIMIT: usize = 120;
pub const MAX_PAYLOAD_BYTES: usize = 32 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub pattern: Option<String>,
    pub idle_ms: Option<u64>,
}

impl Config {
    pub fn validate(&self) -> Result<(), String> {
        if self
            .pattern
            .as_ref()
            .is_some_and(|pattern| pattern.is_empty() || pattern.len() > 4096)
            || self
                .idle_ms
                .is_some_and(|idle| !(10..=60_000).contains(&idle))
            || (self.pattern.is_none() && self.idle_ms.is_none())
        {
            return Err(
                "terminal observation requires a bounded pattern or idle_ms (10..60000)".into(),
            );
        }
        Ok(())
    }
}
