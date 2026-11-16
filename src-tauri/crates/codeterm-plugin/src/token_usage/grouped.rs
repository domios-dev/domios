use super::timestamp::record_timestamp;
use super::{ParsedUsage, TokenCounts};
use serde_json::Value;

pub fn parse_record(value: &Value, now: u64) -> Option<ParsedUsage> {
    let timestamp = record_timestamp(value, now);
    if value.get("summary").and_then(Value::as_bool) == Some(true) {
        return Some(ParsedUsage::Compaction { timestamp });
    }
    Some(ParsedUsage::Response {
        timestamp,
        response_id: value.get("id").and_then(Value::as_str).map(str::to_string),
        tokens: grouped_tokens(value)?,
    })
}

fn grouped_tokens(value: &Value) -> Option<TokenCounts> {
    let tokens = value.get("tokens")?;
    let cache = tokens.get("cache")?;
    let fresh = u64_at(tokens, "input")?;
    let cache_write = u64_at(cache, "write").unwrap_or(0);
    let cache_read = u64_at(cache, "read").unwrap_or(0);
    let uncached = fresh + cache_write;
    Some(TokenCounts {
        input: uncached + cache_read,
        uncached_input: uncached,
        output: u64_at(tokens, "output").unwrap_or(0) + u64_at(tokens, "reasoning").unwrap_or(0),
    })
}

fn u64_at(value: &Value, key: &str) -> Option<u64> {
    value.get(key)?.as_u64()
}
