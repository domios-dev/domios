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
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };

        if let Some(tu) = extract_token_usage(&v) {
            snap.input_tokens = snap.input_tokens.saturating_add(tu.input);
            snap.output_tokens = snap.output_tokens.saturating_add(tu.output);
            snap.cache_creation_input_tokens = snap
                .cache_creation_input_tokens
                .saturating_add(tu.cache_create);
            snap.cache_read_input_tokens =
                snap.cache_read_input_tokens.saturating_add(tu.cache_read);
            snap.message_count = snap.message_count.saturating_add(1);
            found_any = true;

            if let Some(ts) = parse_timestamp(&v) {
                if snap.window_started_at.is_none() {
                    snap.window_started_at = Some(ts);
                }
                snap.last_activity_at = Some(ts);
            }
        }
    }

    if found_any { Some(snap) } else { None }
}

struct TokenCounts {
    input: u64,
    output: u64,
    cache_create: u64,
    cache_read: u64,
}

fn extract_token_usage(v: &Value) -> Option<TokenCounts> {
    let payload = v.get("payload")?;

    if let Some(c) = counts_from_object(payload) {
        return Some(c);
    }

    if let Some(meta) = payload.get("response_meta") {
        if let Some(tu) = meta.get("token_usage") {
            if let Some(c) = counts_from_object(tu) {
                return Some(c);
            }
        }
    }

    if let Some(tu) = payload.get("token_usage") {
        if let Some(c) = counts_from_object(tu) {
            return Some(c);
        }
    }

    if let Some(usage) = payload.get("usage") {
        if let Some(c) = counts_from_object(usage) {
            return Some(c);
        }
    }

    None
}

fn counts_from_object(obj: &Value) -> Option<TokenCounts> {
    let input = u64_field(obj, &["input_tokens", "prompt_tokens", "input"]);
    let output = u64_field(obj, &["output_tokens", "completion_tokens", "output"]);
    let cache_create = u64_field(
        obj,
        &["cache_creation_input_tokens", "cache_creation_tokens", "cache_write_tokens"],
    );
    let cache_read = u64_field(
        obj,
        &["cache_read_input_tokens", "cached_input_tokens", "cache_read_tokens"],
    );

    if input == 0 && output == 0 && cache_create == 0 && cache_read == 0 {
        return None;
    }
    Some(TokenCounts { input, output, cache_create, cache_read })
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

    #[test]
    fn cumulative_sum_across_turns() {
        let dir = TempDir::new().unwrap();
        let path = write_lines(
            &dir,
            "rollout.jsonl",
            &[
                r#"{"timestamp":"2026-05-05T07:14:32.214Z","type":"session_meta","payload":{"id":"abc","cwd":"/tmp"}}"#,
                r#"{"timestamp":"2026-05-05T07:15:00.000Z","type":"event_msg","payload":{"type":"token_count","input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":10,"cache_read_input_tokens":5}}"#,
                r#"{"timestamp":"2026-05-05T07:16:00.000Z","type":"response_item","payload":{"type":"message","role":"assistant","response_meta":{"token_usage":{"input_tokens":200,"output_tokens":75,"cache_creation_input_tokens":20,"cache_read_input_tokens":15}}}}"#,
                r#"{"timestamp":"2026-05-05T07:17:00.000Z","type":"event_msg","payload":{"type":"task_complete"}}"#,
            ],
        );

        let snap = read_from_path(&path).expect("snapshot");
        assert_eq!(snap.input_tokens, 300);
        assert_eq!(snap.output_tokens, 125);
        assert_eq!(snap.cache_creation_input_tokens, 30);
        assert_eq!(snap.cache_read_input_tokens, 20);
        assert_eq!(snap.message_count, 2);
        assert!(snap.window_started_at.is_some());
        assert!(snap.last_activity_at.is_some());
        assert!(snap.last_activity_at.unwrap() >= snap.window_started_at.unwrap());
    }

    #[test]
    fn skips_malformed_lines() {
        let dir = TempDir::new().unwrap();
        let path = write_lines(
            &dir,
            "broken.jsonl",
            &[
                r#"not json at all"#,
                r#"{"timestamp":"2026-05-05T07:15:00.000Z","type":"event_msg","payload":{"type":"token_count","input_tokens":40,"output_tokens":20}}"#,
                r#"{broken json"#,
                r#"{"timestamp":"2026-05-05T07:16:00.000Z","type":"event_msg","payload":{"type":"token_count","input_tokens":60,"output_tokens":30}}"#,
            ],
        );

        let snap = read_from_path(&path).expect("snapshot");
        assert_eq!(snap.input_tokens, 100);
        assert_eq!(snap.output_tokens, 50);
        assert_eq!(snap.message_count, 2);
    }

    #[test]
    fn returns_none_when_no_usage() {
        let dir = TempDir::new().unwrap();
        let path = write_lines(
            &dir,
            "empty.jsonl",
            &[
                r#"{"timestamp":"2026-05-05T07:14:32.214Z","type":"session_meta","payload":{"id":"abc"}}"#,
                r#"{"timestamp":"2026-05-05T07:15:00.000Z","type":"event_msg","payload":{"type":"task_started"}}"#,
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
}
