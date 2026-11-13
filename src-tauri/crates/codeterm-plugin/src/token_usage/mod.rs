use serde_json::Value;

pub mod cumulative;
pub mod grouped;
pub mod nested;
mod timestamp;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TokenCounts {
    pub input: u64,
    pub uncached_input: u64,
    pub output: u64,
}

impl TokenCounts {
    pub fn total(&self) -> u64 {
        self.input.saturating_add(self.output)
    }

    pub fn max_each(self, other: TokenCounts) -> TokenCounts {
        TokenCounts {
            input: self.input.max(other.input),
            uncached_input: self.uncached_input.max(other.uncached_input),
            output: self.output.max(other.output),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CumulativeUsage {
    pub input: u64,
    pub cached_input: u64,
    pub output: u64,
    pub total: u64,
}

impl CumulativeUsage {
    pub fn as_tokens(self) -> TokenCounts {
        TokenCounts {
            input: self.input,
            uncached_input: self.input.saturating_sub(self.cached_input),
            output: self.output,
        }
    }

    /// `None` when nothing advanced; a counter that went backwards starts a new run from zero.
    pub fn advance_from(self, previous: CumulativeUsage) -> Option<TokenCounts> {
        if self == previous {
            return None;
        }
        let monotonic = self.input >= previous.input
            && self.cached_input >= previous.cached_input
            && self.output >= previous.output
            && self.total >= previous.total;
        if !monotonic {
            return Some(self.as_tokens());
        }
        let input = self.input - previous.input;
        let cached = self.cached_input - previous.cached_input;
        Some(TokenCounts {
            input,
            uncached_input: input.saturating_sub(cached),
            output: self.output - previous.output,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ParsedUsage {
    Compaction {
        timestamp: u64,
    },
    Response {
        timestamp: u64,
        response_id: Option<String>,
        tokens: TokenCounts,
    },
    Cumulative {
        timestamp: u64,
        total: CumulativeUsage,
        last: Option<TokenCounts>,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageAccounting {
    Transcript,
    RuntimeTurn,
    #[default]
    Unreported,
}

pub fn parse_archive_record(value: &Value) -> Result<Option<ParsedUsage>, String> {
    if value.get("usage_contract").and_then(Value::as_u64) != Some(1) {
        return Ok(None);
    }
    serde_json::from_value(value["usage"].clone())
        .map(Some)
        .map_err(|e| e.to_string())
}
