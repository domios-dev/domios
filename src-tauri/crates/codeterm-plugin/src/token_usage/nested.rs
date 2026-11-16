use super::timestamp::record_timestamp;
use super::{ParsedUsage, TokenCounts};
use serde_json::Value;

pub fn parse_record(value: &Value, now: u64) -> Option<ParsedUsage> {
    let timestamp = record_timestamp(value, now);
    if value.get("compactMetadata").is_some_and(Value::is_object) {
        return Some(ParsedUsage::Compaction { timestamp });
    }
    Some(ParsedUsage::Response {
        timestamp,
        response_id: nested_response_id(value),
        tokens: nested_usage_tokens(value)?,
    })
}

fn nested_usage_tokens(value: &Value) -> Option<TokenCounts> {
    let usage = value.get("message")?.get("usage")?;
    let fresh = u64_at(usage, "input_tokens")?;
    let cache_write = u64_at(usage, "cache_creation_input_tokens").unwrap_or(0);
    let cache_read = u64_at(usage, "cache_read_input_tokens").unwrap_or(0);
    let uncached = fresh + cache_write;
    Some(TokenCounts {
        input: uncached + cache_read,
        uncached_input: uncached,
        output: u64_at(usage, "output_tokens").unwrap_or(0),
    })
}

fn nested_response_id(value: &Value) -> Option<String> {
    let message_id = value.get("message")?.get("id")?.as_str()?;
    let request_id = value
        .get("requestId")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some(format!("{message_id}:{request_id}"))
}

fn u64_at(value: &Value, key: &str) -> Option<u64> {
    value.get(key)?.as_u64()
}
