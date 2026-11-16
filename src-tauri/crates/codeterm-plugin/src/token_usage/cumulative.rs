use super::timestamp::record_timestamp;
use super::{ParsedUsage, TokenCounts};
use serde_json::Value;

use super::CumulativeUsage;

pub fn parse_record(value: &Value, now: u64) -> Option<ParsedUsage> {
    let timestamp = record_timestamp(value, now);
    if value.get("type").and_then(Value::as_str) == Some("compacted") {
        return Some(ParsedUsage::Compaction { timestamp });
    }
    let info = value.get("payload")?.get("info")?;
    let last = last_turn_usage_tokens(info);
    if let Some(total) = cumulative_usage(info) {
        return Some(ParsedUsage::Cumulative {
            timestamp,
            total,
            last,
        });
    }
    Some(ParsedUsage::Response {
        timestamp,
        response_id: None,
        tokens: last?,
    })
}

fn last_turn_usage_tokens(info: &Value) -> Option<TokenCounts> {
    let usage = info.get("last_token_usage")?;
    let input = u64_at(usage, "input_tokens")?;
    let cached = u64_at(usage, "cached_input_tokens").unwrap_or(0);
    Some(TokenCounts {
        input,
        uncached_input: input.saturating_sub(cached),
        output: u64_at(usage, "output_tokens").unwrap_or(0),
    })
}

fn cumulative_usage(info: &Value) -> Option<CumulativeUsage> {
    let usage = info.get("total_token_usage")?;
    let input = u64_at(usage, "input_tokens")?;
    let output = u64_at(usage, "output_tokens").unwrap_or(0);
    Some(CumulativeUsage {
        input,
        cached_input: u64_at(usage, "cached_input_tokens").unwrap_or(0),
        output,
        total: u64_at(usage, "total_tokens").unwrap_or(input + output),
    })
}

fn u64_at(value: &Value, key: &str) -> Option<u64> {
    value.get(key)?.as_u64()
}
