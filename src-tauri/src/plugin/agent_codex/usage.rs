//! Token usage parsing for Codex's per-session JSONL rollouts.
//!
//! Real Codex (codex-cli >= 0.128) emits one `event_msg/token_count` event
//! per turn carrying the *running cumulative total* at
//! `payload.info.total_token_usage`. Subsequent events overwrite the snapshot;
//! we never sum across events for that shape — that would multiply the
//! totals by the event count.
//!
//! Cache-token field name in the wild: `cached_input_tokens` (no
//! `cache_creation_input_tokens` — Codex doesn't model cache writes
//! separately, the model's input includes both fresh and cached tokens).
//!
//! Verified live against real Mac sessions (108 events, final total
//! input_tokens=366721) — the file existed but the parser previously
//! returned None for it. See PR #54.

use std::io::{BufRead, BufReader};
use std::path::Path;

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::agent::provider::UsageSnapshot;

pub fn read_from_path(path: &Path) -> Option<UsageSnapshot> {
    let file = std::fs::File::open(path).ok()?;
    let reader = BufReader::new(file);

    let mut snap = UsageSnapshot::default();
    let mut found_any = false;

    for line in reader.lines().map_while(Result::ok) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };

        let Some(counts) = extract_token_usage(&v) else {
            continue;
        };

        snap.input_tokens = counts.input;
        snap.output_tokens = counts.output;
        snap.cache_creation_input_tokens = counts.cache_create;
        snap.cache_read_input_tokens = counts.cache_read;
        snap.message_count = snap.message_count.saturating_add(1);
        found_any = true;

        if let Some(ts) = parse_timestamp(&v) {
            if snap.window_started_at.is_none() {
                snap.window_started_at = Some(ts);
            }
            snap.last_activity_at = Some(ts);
        }
    }

    if found_any {
        Some(snap)
    } else {
        None
    }
}

struct TokenCounts {
    input: u64,
    output: u64,
    cache_create: u64,
    cache_read: u64,
}

fn extract_token_usage(v: &Value) -> Option<TokenCounts> {
    let payload = v.get("payload")?;

    if let Some(info) = payload.get("info") {
        if let Some(tu) = info
            .get("total_token_usage")
            .or_else(|| info.get("last_token_usage"))
        {
            if let Some(c) = counts_from_object(tu) {
                return Some(c);
            }
        }
    }

    None
}

fn counts_from_object(obj: &Value) -> Option<TokenCounts> {
    let input = u64_field(obj, &["input_tokens", "prompt_tokens"]);
    let output = u64_field(obj, &["output_tokens", "completion_tokens"]);
    let cache_read = u64_field(obj, &["cached_input_tokens", "cache_read_input_tokens"]);
    let cache_create = u64_field(obj, &["cache_creation_input_tokens"]);

    if input == 0 && output == 0 && cache_create == 0 && cache_read == 0 {
        return None;
    }
    Some(TokenCounts {
        input,
        output,
        cache_create,
        cache_read,
    })
}

fn u64_field(obj: &Value, candidates: &[&str]) -> u64 {
    for key in candidates {
        if let Some(n) = obj.get(*key).and_then(|v| v.as_u64()) {
            return n;
        }
    }
    0
}

fn parse_timestamp(v: &Value) -> Option<DateTime<Utc>> {
    let raw = v.get("timestamp").and_then(|t| t.as_str())?;
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn write_lines(dir: &TempDir, name: &str, lines: &[&str]) -> std::path::PathBuf {
        let path = dir.path().join(name);
        let mut f = std::fs::File::create(&path).expect("create fixture");
        for l in lines {
            writeln!(f, "{}", l).expect("write");
        }
        path
    }

    /// Mirrors the real codex-cli >= 0.128 event_msg/token_count shape.
    /// Verified against ~/.codex/sessions/YYYY/MM/DD/
    /// rollout-...jsonl — last event_msg carries the cumulative total.
    #[test]
    fn takes_last_event_total_not_sum() {
        let dir = TempDir::new().unwrap();
        let path = write_lines(
            &dir,
            "rollout.jsonl",
            &[
                r#"{"timestamp":"2026-05-05T07:14:32.214Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":3577,"output_tokens":628,"cached_input_tokens":2560}}}}"#,
                r#"{"timestamp":"2026-05-05T07:15:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":7866,"output_tokens":692,"cached_input_tokens":5120}}}}"#,
                r#"{"timestamp":"2026-05-05T07:16:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":366721,"output_tokens":2238,"cached_input_tokens":265000}}}}"#,
            ],
        );

        let snap = read_from_path(&path).expect("snapshot");
        // Expect the LAST event's totals, not the sum (which would be 378164).
        assert_eq!(snap.input_tokens, 366721, "should be last total, not sum");
        assert_eq!(snap.output_tokens, 2238, "should be last total, not sum");
        assert_eq!(snap.cache_read_input_tokens, 265000);
        assert_eq!(
            snap.cache_creation_input_tokens, 0,
            "codex doesn't model cache writes"
        );
        assert_eq!(snap.message_count, 3, "count of token_count events seen");
        assert!(snap.window_started_at.is_some());
        assert!(snap.last_activity_at >= snap.window_started_at);
    }

    #[test]
    fn falls_back_to_last_token_usage_when_total_absent() {
        let dir = TempDir::new().unwrap();
        let path = write_lines(
            &dir,
            "no_total.jsonl",
            &[
                r#"{"timestamp":"2026-05-05T07:15:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":42,"output_tokens":13}}}}"#,
            ],
        );
        let snap = read_from_path(&path).expect("snapshot");
        assert_eq!(snap.input_tokens, 42);
        assert_eq!(snap.output_tokens, 13);
    }

    #[test]
    fn skips_malformed_lines() {
        let dir = TempDir::new().unwrap();
        let path = write_lines(
            &dir,
            "broken.jsonl",
            &[
                r#"not json at all"#,
                r#"{"timestamp":"2026-05-05T07:15:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":40,"output_tokens":20,"cached_input_tokens":5}}}}"#,
                r#"{broken json"#,
                r#"{"timestamp":"2026-05-05T07:16:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":80,"output_tokens":40,"cached_input_tokens":10}}}}"#,
            ],
        );

        let snap = read_from_path(&path).expect("snapshot");
        assert_eq!(snap.input_tokens, 80, "last valid event wins");
        assert_eq!(snap.output_tokens, 40);
        assert_eq!(snap.message_count, 2);
    }

    #[test]
    fn returns_none_when_no_token_count_events() {
        let dir = TempDir::new().unwrap();
        let path = write_lines(
            &dir,
            "empty.jsonl",
            &[
                r#"{"timestamp":"2026-05-05T07:14:32.214Z","type":"session_meta","payload":{"id":"abc"}}"#,
                r#"{"timestamp":"2026-05-05T07:15:00.000Z","type":"event_msg","payload":{"type":"task_started"}}"#,
                r#"{"timestamp":"2026-05-05T07:16:00.000Z","type":"response_item","payload":{"type":"message","role":"user"}}"#,
            ],
        );

        assert!(read_from_path(&path).is_none());
    }

    #[test]
    fn returns_none_when_file_missing() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("does-not-exist.jsonl");
        assert!(read_from_path(&missing).is_none());
    }

    #[test]
    fn ignores_zero_only_total() {
        let dir = TempDir::new().unwrap();
        let path = write_lines(
            &dir,
            "zero.jsonl",
            &[
                r#"{"timestamp":"2026-05-05T07:14:32.214Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":0,"output_tokens":0,"cached_input_tokens":0}}}}"#,
            ],
        );
        assert!(
            read_from_path(&path).is_none(),
            "all-zero token block carries no signal"
        );
    }
}
