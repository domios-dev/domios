//! Structural recognition of provider usage records. Each shape is identified by the
//! fields it carries, never by a provider id or vendor copy — a provider that changes
//! its wrapper keys is a parse miss, not a mis-attribution.

use serde_json::Value;

use crate::agent::kpi_ledger::TokenCounts;

/// `tokens: None` is a context-compaction marker: an event with no token cost of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsageRecord {
    pub timestamp: u64,
    pub tokens: Option<TokenCounts>,
}

pub fn parse_record(value: &Value) -> Option<UsageRecord> {
    let timestamp = record_timestamp(value);
    if is_compaction(value) {
        return Some(UsageRecord {
            timestamp,
            tokens: None,
        });
    }
    let tokens = assistant_turn_tokens(value)?;
    Some(UsageRecord {
        timestamp,
        tokens: Some(tokens),
    })
}

/// Dropped-context metadata alongside a boundary marker is the fingerprint; the
/// human-readable `content` string is deliberately not matched.
fn is_compaction(value: &Value) -> bool {
    value.get("compactMetadata").is_some_and(Value::is_object)
        || is_codex_compaction(value)
        || is_opencode_compaction(value)
}

/// Codex CLI's rollout `RolloutItem` enum is serialized `#[serde(tag = "type", content
/// = "payload")]`; `"compacted"` is the wire tag for `RolloutItem::Compacted`, a
/// protocol discriminant fixed by `codex-rs/protocol/src/protocol.rs`, not vendor copy.
fn is_codex_compaction(value: &Value) -> bool {
    value.get("type").and_then(Value::as_str) == Some("compacted")
}

/// OpenCode stamps a context-summary turn with `summary: true` on the assistant
/// message row itself (`Assistant.summary: Schema.optional(Schema.Boolean)` in
/// `packages/schema/src/v1/session.ts`) — distinct from the user-message `summary`
/// object field, so a plain boolean check can't collide with it.
fn is_opencode_compaction(value: &Value) -> bool {
    value.get("summary").and_then(Value::as_bool) == Some(true)
}

fn assistant_turn_tokens(value: &Value) -> Option<TokenCounts> {
    nested_usage_tokens(value)
        .or_else(|| last_turn_usage_tokens(value))
        .or_else(|| grouped_tokens(value))
}

/// Shape: `message.usage.{input_tokens, cache_creation_input_tokens,
/// cache_read_input_tokens, output_tokens}` — cache reads are the cached slice.
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

/// Shape: `payload.info.last_token_usage.{input_tokens, cached_input_tokens,
/// output_tokens}` — `input_tokens` already includes the cached slice here.
fn last_turn_usage_tokens(value: &Value) -> Option<TokenCounts> {
    let usage = value.get("payload")?.get("info")?.get("last_token_usage")?;
    let input = u64_at(usage, "input_tokens")?;
    let cached = u64_at(usage, "cached_input_tokens").unwrap_or(0);
    Some(TokenCounts {
        input,
        uncached_input: input.saturating_sub(cached),
        output: u64_at(usage, "output_tokens").unwrap_or(0),
    })
}

/// Shape: `tokens.{input, output, reasoning, cache:{read, write}}` — reasoning is a
/// sibling of `output` in this shape, so it is added rather than assumed contained.
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

fn record_timestamp(value: &Value) -> u64 {
    if let Some(secs) = value.get("timestamp").and_then(rfc3339_secs) {
        return secs;
    }
    if let Some(ms) = value
        .get("time")
        .and_then(|t| t.get("created"))
        .and_then(Value::as_u64)
    {
        return ms / 1000;
    }
    super::super::attribution::now_epoch_secs()
}

/// Minimal RFC3339 → unix seconds. Provider logs stamp UTC `Z` timestamps; anything
/// else falls through to the ingest clock rather than guessing an offset.
fn rfc3339_secs(value: &Value) -> Option<u64> {
    if let Some(secs) = value.as_u64() {
        return Some(secs);
    }
    let (date, rest) = value.as_str()?.split_once('T')?;
    let time = rest.strip_suffix('Z')?;
    let mut date_parts = date.split('-');
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month: i64 = date_parts.next()?.parse().ok()?;
    let day: i64 = date_parts.next()?.parse().ok()?;
    let mut time_parts = time.split(':');
    let hour: i64 = time_parts.next()?.parse().ok()?;
    let minute: i64 = time_parts.next()?.parse().ok()?;
    let second: i64 = time_parts.next()?.split('.').next()?.parse::<f64>().ok()? as i64;
    let days = days_from_civil(year, month, day);
    let secs = days * 86_400 + hour * 3_600 + minute * 60 + second;
    u64::try_from(secs).ok()
}

/// Howard Hinnant's civil-from-days inverse; avoids a chrono dependency for one field.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}
