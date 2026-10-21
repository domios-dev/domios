//! Token usage parsing for Claude's per-session JSONL logs.
//!
//! Each line in `~/.claude/projects/<encoded>/<session>.jsonl` is one event.
//! Token counts live at `message.usage.{input,output,cache_creation_input,
//! cache_read_input}_tokens`. Timestamp at top-level `timestamp` (RFC3339).
//! Totals fold forward from a per-file offset, so a call reads only appended bytes.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
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
    probe: u64,
}

#[derive(Default)]
struct UsageCursors {
    cursors: Mutex<VecDeque<(PathBuf, UsageCursor)>>,
}

impl UsageCursors {
    fn read(&self, path: &Path) -> UsageSnapshot {
        let opened = std::fs::File::open(path)
            .and_then(|file| file.metadata().map(|metadata| (file, metadata.len())));
        let Ok((mut file, length)) = opened else {
            self.cursors.lock().retain(|(key, _)| key != path);
            return UsageSnapshot::default();
        };
        let mut cursors = self.cursors.lock();
        let cached = cursors
            .iter()
            .position(|(key, _)| key == path)
            .and_then(|index| cursors.remove(index))
            .map(|(_, cursor)| cursor);
        let mut cursor = cached
            .filter(|cursor| {
                cursor.offset <= length
                    && fingerprint(&mut file, cursor.offset).ok() == Some(cursor.probe)
            })
            .unwrap_or_default();
        if cursor.offset < length && file.seek(SeekFrom::Start(cursor.offset)).is_ok() {
            cursor.offset += fold(&mut cursor.snap, BufReader::new(&mut file), path);
        }
        let snap = cursor.snap.clone();
        if let Ok(probe) = fingerprint(&mut file, cursor.offset) {
            cursor.probe = probe;
            cursors.push_back((path.to_owned(), cursor));
            while cursors.len() > MAX_CURSORS {
                cursors.pop_front();
            }
        }
        snap
    }

    #[cfg(test)]
    fn offset(&self, path: &Path) -> Option<u64> {
        self.cursors
            .lock()
            .iter()
            .find(|(key, _)| key == path)
            .map(|(_, cursor)| cursor.offset)
    }
}

fn fingerprint(file: &mut std::fs::File, end: u64) -> std::io::Result<u64> {
    use std::hash::{Hash, Hasher};
    let start = end.saturating_sub(4096);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = vec![0; (end - start) as usize];
    file.read_exact(&mut bytes)?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    Ok(hasher.finish())
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
mod tests;
