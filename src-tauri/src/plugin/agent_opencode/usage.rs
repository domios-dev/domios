use std::path::Path;

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::agent::provider::UsageSnapshot;

/// Parse cumulative usage from a directory of opencode message JSON files
/// (`<storage>/message/<session_id>/msg_*.json`). Returns None when the
/// directory has no usage-bearing assistant messages.
pub fn read_from_path(messages_dir: &Path) -> Option<UsageSnapshot> {
    let entries = std::fs::read_dir(messages_dir).ok()?;

    let mut rows: Vec<(Option<i64>, Value)> = Vec::new();
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if !name.starts_with("msg_") || !name.ends_with(".json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let ts = v
            .get("time")
            .and_then(|t| t.get("created"))
            .and_then(|c| c.as_i64());
        rows.push((ts, v));
    }

    rows.sort_by_key(|(ts, _)| ts.unwrap_or(i64::MAX));
    accumulate(rows.into_iter().map(|(_, v)| v))
}

/// Accumulate usage from an iterator of opencode message JSON values.
/// Sqlite-backed callers route DB rows through this same path so both
/// backends share the field-extraction logic.
pub fn accumulate<I: Iterator<Item = Value>>(messages: I) -> Option<UsageSnapshot> {
    let mut snap = UsageSnapshot::default();
    let mut found_any = false;

    for v in messages {
        let role = v.get("role").and_then(|r| r.as_str()).unwrap_or("");
        if role != "assistant" {
            continue;
        }
        let Some(tokens) = v.get("tokens") else {
            continue;
        };
        let input = tokens.get("input").and_then(|n| n.as_u64()).unwrap_or(0);
        let output = tokens.get("output").and_then(|n| n.as_u64()).unwrap_or(0);
        let cache_read = tokens
            .pointer("/cache/read")
            .and_then(|n| n.as_u64())
            .unwrap_or(0);
        let cache_write = tokens
            .pointer("/cache/write")
            .and_then(|n| n.as_u64())
            .unwrap_or(0);

        if input == 0 && output == 0 && cache_read == 0 && cache_write == 0 {
            continue;
        }

        snap.input_tokens = snap.input_tokens.saturating_add(input);
        snap.output_tokens = snap.output_tokens.saturating_add(output);
        snap.cache_creation_input_tokens =
            snap.cache_creation_input_tokens.saturating_add(cache_write);
        snap.cache_read_input_tokens = snap.cache_read_input_tokens.saturating_add(cache_read);
        snap.message_count = snap.message_count.saturating_add(1);
        found_any = true;

        if let Some(ts) = ts_from_message(&v) {
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

fn ts_from_message(v: &Value) -> Option<DateTime<Utc>> {
    let ms = v
        .get("time")
        .and_then(|t| t.get("completed").or_else(|| t.get("created")))
        .and_then(|c| c.as_i64())?;
    DateTime::<Utc>::from_timestamp_millis(ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn write_msg(dir: &Path, name: &str, body: &str) {
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).expect("create msg file");
        f.write_all(body.as_bytes()).expect("write msg body");
    }

    #[test]
    fn cumulative_sum_assistant_only() {
        let dir = TempDir::new().unwrap();
        let messages = dir.path();

        write_msg(
            messages,
            "msg_user001.json",
            r#"{"id":"msg_user001","role":"user","time":{"created":1000}}"#,
        );
        write_msg(
            messages,
            "msg_assist001.json",
            r#"{
                "id":"msg_assist001","role":"assistant",
                "time":{"created":1100,"completed":1200},
                "tokens":{"input":50,"output":120,"reasoning":0,"cache":{"read":300,"write":40}}
            }"#,
        );
        write_msg(
            messages,
            "msg_assist002.json",
            r#"{
                "id":"msg_assist002","role":"assistant",
                "time":{"created":2100,"completed":2400},
                "tokens":{"input":75,"output":200,"cache":{"read":500,"write":60}}
            }"#,
        );

        let snap = read_from_path(messages).expect("snapshot");
        assert_eq!(snap.input_tokens, 125);
        assert_eq!(snap.output_tokens, 320);
        assert_eq!(snap.cache_creation_input_tokens, 100);
        assert_eq!(snap.cache_read_input_tokens, 800);
        assert_eq!(snap.message_count, 2);
        let started = snap.window_started_at.expect("window start");
        let last = snap.last_activity_at.expect("last activity");
        assert!(last >= started);
    }

    #[test]
    fn skips_malformed_files() {
        let dir = TempDir::new().unwrap();
        let messages = dir.path();

        write_msg(messages, "msg_broken.json", "not json");
        write_msg(
            messages,
            "msg_assist001.json",
            r#"{"id":"msg_assist001","role":"assistant","time":{"created":1100,"completed":1200},"tokens":{"input":40,"output":20,"cache":{"read":0,"write":0}}}"#,
        );
        write_msg(
            messages,
            "msg_no_tokens.json",
            r#"{"id":"msg_x","role":"assistant","time":{"created":1300}}"#,
        );

        let snap = read_from_path(messages).expect("snapshot");
        assert_eq!(snap.input_tokens, 40);
        assert_eq!(snap.output_tokens, 20);
        assert_eq!(snap.message_count, 1);
    }

    #[test]
    fn returns_none_when_no_assistant_with_tokens() {
        let dir = TempDir::new().unwrap();
        let messages = dir.path();
        write_msg(
            messages,
            "msg_user001.json",
            r#"{"id":"msg_user001","role":"user","time":{"created":1000}}"#,
        );
        assert!(read_from_path(messages).is_none());
    }

    #[test]
    fn returns_none_when_dir_missing() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("nope");
        assert!(read_from_path(&missing).is_none());
    }

    #[test]
    #[ignore = "real-data sanity check; opt in with --ignored"]
    fn real_session_via_provider() {
        use crate::agent::provider::AgentProvider;
        use crate::plugin::agent_opencode::OpenCodeProvider;
        let p = OpenCodeProvider::new();
        let Some(snap) = p.read_usage("", "ses_3a8a72f07ffeF6YZ7vjD3f1kHo") else {
            eprintln!("no local opencode data; skipping");
            return;
        };
        eprintln!(
            "input={} output={} cache_w={} cache_r={} msgs={}",
            snap.input_tokens,
            snap.output_tokens,
            snap.cache_creation_input_tokens,
            snap.cache_read_input_tokens,
            snap.message_count
        );
        assert!(snap.message_count > 0);
    }
}
