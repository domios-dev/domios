//! Token usage parsing for Claude's per-session JSONL logs.
//!
//! Each line in `~/.claude/projects/<encoded>/<session>.jsonl` is one event.
//! Token counts live at `message.usage.{input,output,cache_creation_input,
//! cache_read_input}_tokens`. Timestamp at top-level `timestamp` (RFC3339).
//! Totals fold forward from a per-file offset, so a call reads only appended bytes.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use parking_lot::Mutex;

use crate::provider::UsageSnapshot;

use super::sessions;

const MAX_CURSORS: usize = 64;

static CURSORS: LazyLock<UsageCursors> = LazyLock::new(UsageCursors::default);

pub(super) fn read(cwd: &str, session_id: &str) -> Option<UsageSnapshot> {
    let path = sessions::session_file_path(cwd, session_id)?;
    if !path.exists() {
        return None;
    }
    Some(CURSORS.read(&path))
}

#[derive(Clone, Default)]
struct UsageCursor {
    offset: u64,
    snap: UsageSnapshot,
}

#[derive(Default)]
struct UsageCursors {
    cursors: Mutex<HashMap<PathBuf, UsageCursor>>,
}

impl UsageCursors {
    fn read(&self, path: &Path) -> UsageSnapshot {
        let opened = std::fs::File::open(path)
            .and_then(|file| file.metadata().map(|metadata| (file, metadata.len())));
        let Ok((mut file, length)) = opened else {
            self.cursors.lock().remove(path);
            return UsageSnapshot::default();
        };
        let mut cursor = self
            .cursors
            .lock()
            .get(path)
            .filter(|cursor| cursor.offset <= length)
            .cloned()
            .unwrap_or_default();
        if cursor.offset < length && file.seek(SeekFrom::Start(cursor.offset)).is_ok() {
            cursor.offset += fold(&mut cursor.snap, BufReader::new(file), path);
        }
        let snap = cursor.snap.clone();
        let mut cursors = self.cursors.lock();
        if cursors.len() >= MAX_CURSORS && !cursors.contains_key(path) {
            cursors.clear();
        }
        cursors.insert(path.to_owned(), cursor);
        snap
    }

    #[cfg(test)]
    fn offset(&self, path: &Path) -> Option<u64> {
        self.cursors.lock().get(path).map(|cursor| cursor.offset)
    }
}

#[cfg(test)]
fn read_from_path(path: &Path) -> UsageSnapshot {
    UsageCursors::default().read(path)
}

/// Folds complete lines into `snap`; a trailing partial line stays unread.
fn fold(snap: &mut UsageSnapshot, mut reader: impl BufRead, path: &Path) -> u64 {
    let mut consumed = 0;
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        let Ok(count) = reader.read_until(b'\n', &mut bytes) else {
            break;
        };
        if count == 0 || bytes.last() != Some(&b'\n') {
            break;
        }
        consumed += count as u64;
        let line = String::from_utf8_lossy(&bytes);
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                log::debug!(
                    "[claude-usage] skip malformed line in {}: {}",
                    path.display(),
                    e
                );
                continue;
            }
        };

        let Some(usage) = v.get("message").and_then(|m| m.get("usage")) else {
            continue;
        };

        snap.input_tokens += token_field(usage, "input_tokens");
        snap.output_tokens += token_field(usage, "output_tokens");
        snap.cache_creation_input_tokens += token_field(usage, "cache_creation_input_tokens");
        snap.cache_read_input_tokens += token_field(usage, "cache_read_input_tokens");
        snap.message_count += 1;

        if let Some(ts) = v
            .get("timestamp")
            .and_then(|t| t.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc))
        {
            if snap.window_started_at.is_none() {
                snap.window_started_at = Some(ts);
            }
            snap.last_activity_at = Some(ts);
        }
    }
    consumed
}

fn token_field(usage: &serde_json::Value, name: &str) -> u64 {
    usage.get(name).and_then(|v| v.as_u64()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_fixture(dir: &Path, name: &str, content: &str) -> std::path::PathBuf {
        let path = dir.join(format!("{}.jsonl", name));
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn empty_file_returns_zero_snapshot() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_fixture(tmp.path(), "empty", "");
        let snap = read_from_path(&path);
        assert_eq!(snap, UsageSnapshot::default());
    }

    #[test]
    fn missing_file_returns_default_snapshot() {
        let tmp = tempfile::tempdir().unwrap();
        let snap = read_from_path(&tmp.path().join("nonexistent.jsonl"));
        assert_eq!(snap, UsageSnapshot::default());
    }

    #[test]
    fn sums_cumulative_across_messages() {
        let tmp = tempfile::tempdir().unwrap();
        let content = concat!(
            r#"{"timestamp":"2026-05-07T10:00:00Z","message":{"usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":10,"cache_read_input_tokens":5}}}"#,
            "\n",
            r#"{"timestamp":"2026-05-07T10:01:00Z","message":{"usage":{"input_tokens":200,"output_tokens":75,"cache_creation_input_tokens":20,"cache_read_input_tokens":15}}}"#,
            "\n",
            r#"{"timestamp":"2026-05-07T10:02:00Z","type":"system_event"}"#,
            "\n",
        );
        let path = write_fixture(tmp.path(), "session", content);
        let snap = read_from_path(&path);

        assert_eq!(snap.input_tokens, 300);
        assert_eq!(snap.output_tokens, 125);
        assert_eq!(snap.cache_creation_input_tokens, 30);
        assert_eq!(snap.cache_read_input_tokens, 20);
        assert_eq!(
            snap.message_count, 2,
            "system_event line has no usage block"
        );
    }

    #[test]
    fn skips_malformed_lines_without_panic() {
        let tmp = tempfile::tempdir().unwrap();
        let content = concat!(
            r#"{"timestamp":"2026-05-07T10:00:00Z","message":{"usage":{"input_tokens":10,"output_tokens":5,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#,
            "\n",
            "this is not json at all\n",
            r#"{"unterminated":"#,
            "\n",
            r#"{"timestamp":"2026-05-07T10:01:00Z","message":{"usage":{"input_tokens":20,"output_tokens":10,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#,
            "\n",
        );
        let path = write_fixture(tmp.path(), "messy", content);
        let snap = read_from_path(&path);
        assert_eq!(snap.input_tokens, 30);
        assert_eq!(snap.output_tokens, 15);
        assert_eq!(snap.message_count, 2);
    }

    #[test]
    fn captures_first_and_last_timestamps() {
        let tmp = tempfile::tempdir().unwrap();
        let content = concat!(
            r#"{"timestamp":"2026-05-07T10:00:00Z","message":{"usage":{"input_tokens":1,"output_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#,
            "\n",
            r#"{"timestamp":"2026-05-07T10:30:00Z","message":{"usage":{"input_tokens":1,"output_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#,
            "\n",
            r#"{"timestamp":"2026-05-07T11:15:00Z","message":{"usage":{"input_tokens":1,"output_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#,
            "\n",
        );
        let path = write_fixture(tmp.path(), "timestamped", content);
        let snap = read_from_path(&path);

        let first = snap.window_started_at.expect("first ts present");
        let last = snap.last_activity_at.expect("last ts present");
        assert_eq!(first.to_rfc3339(), "2026-05-07T10:00:00+00:00");
        assert_eq!(last.to_rfc3339(), "2026-05-07T11:15:00+00:00");
    }

    #[test]
    fn missing_token_fields_default_to_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let content = concat!(
            r#"{"timestamp":"2026-05-07T10:00:00Z","message":{"usage":{"input_tokens":42}}}"#,
            "\n",
        );
        let path = write_fixture(tmp.path(), "partial", content);
        let snap = read_from_path(&path);
        assert_eq!(snap.input_tokens, 42);
        assert_eq!(snap.output_tokens, 0);
        assert_eq!(snap.cache_creation_input_tokens, 0);
        assert_eq!(snap.cache_read_input_tokens, 0);
        assert_eq!(snap.message_count, 1);
    }

    fn usage_line(input: u64) -> String {
        format!(
            "{{\"timestamp\":\"2026-05-07T10:00:00Z\",\"message\":{{\"usage\":{{\"input_tokens\":{input}}}}}}}\n"
        )
    }

    #[test]
    fn rereads_fold_only_appended_complete_lines() {
        use std::io::Write;
        let tmp = tempfile::tempdir().unwrap();
        let first = usage_line(10);
        let path = write_fixture(tmp.path(), "growing", &first);
        let cursors = UsageCursors::default();
        assert_eq!(cursors.read(&path).input_tokens, 10);
        assert_eq!(cursors.offset(&path), Some(first.len() as u64));

        let second = usage_line(5);
        let (complete, partial) = second.split_at(second.len() - 3);
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(complete.as_bytes()).unwrap();
        let held = cursors.read(&path);
        assert_eq!(held.input_tokens, 10);
        assert_eq!(cursors.offset(&path), Some(first.len() as u64));

        file.write_all(partial.as_bytes()).unwrap();
        let folded = cursors.read(&path);
        assert_eq!(folded.input_tokens, 15);
        assert_eq!(folded.message_count, 2);
        assert_eq!(
            cursors.offset(&path),
            Some((first.len() + second.len()) as u64)
        );
        assert_eq!(cursors.read(&path), folded);
    }

    #[test]
    fn a_truncated_file_refolds_from_the_start() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_fixture(tmp.path(), "rewritten", &(usage_line(7) + &usage_line(8)));
        let cursors = UsageCursors::default();
        assert_eq!(cursors.read(&path).input_tokens, 15);
        std::fs::write(&path, usage_line(3)).unwrap();
        let snap = cursors.read(&path);
        assert_eq!(snap.input_tokens, 3);
        assert_eq!(snap.message_count, 1);
    }
}
