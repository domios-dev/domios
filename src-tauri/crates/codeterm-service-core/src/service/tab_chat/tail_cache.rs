//! Incremental bounded tails of session JSONL files: readers of the recent
//! window share one parsed tail per file, so a re-read costs only appended bytes.

use std::collections::VecDeque;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;

use super::session_window::{self, SNAPSHOT_WINDOW_BYTES};
use super::snapshot_rows::{retained_group_start, SNAPSHOT_ROW_LIMIT};
use crate::agent::chat_types::ChatMsg;
use crate::agent::registry::AgentRegistry;

const MAX_ENTRIES: usize = 32;
const MAX_BYTES: usize = 32 * 1024 * 1024;
const PROBE_BYTES: u64 = 4096;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TailRows {
    pub messages: Vec<ChatMsg>,
    /// Source record start of each message, parallel to `messages`.
    pub record_starts: Vec<u64>,
    pub start_offset: u64,
    pub end_offset: u64,
}

/// File access the cache needs; production reads the JSONL through the
/// provider, tests inject an in-memory transcript.
pub trait TailPort {
    fn length(&self) -> Option<u64>;
    /// Fingerprint of the bytes just before `end`; a mismatch means the file
    /// was rewritten rather than appended.
    fn probe(&self, end: u64) -> Option<u64>;
    /// Fresh bounded window and the transcript bytes it parsed.
    fn window(&self) -> (TailRows, u64);
    /// Complete records in `from..until` and the transcript bytes parsed.
    fn append(&self, from: u64, until: u64) -> (TailRows, u64);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TailPlan {
    Reuse,
    Append { from: u64 },
    Rebuild,
}

impl TailPlan {
    fn label(self) -> &'static str {
        match self {
            Self::Reuse => "reuse",
            Self::Append { .. } => "append",
            Self::Rebuild => "rebuild",
        }
    }
}

pub fn plan(cached_total: u64, cached_end: u64, total: u64, probe_matches: bool) -> TailPlan {
    if !probe_matches || total < cached_total || cached_end > total {
        return TailPlan::Rebuild;
    }
    if total == cached_total {
        return TailPlan::Reuse;
    }
    if total - cached_end >= SNAPSHOT_WINDOW_BYTES {
        return TailPlan::Rebuild;
    }
    TailPlan::Append { from: cached_end }
}

/// Append `appended` to `cached` and keep what a fresh window would: records
/// starting inside the trailing window, whole, within the row limit.
pub fn merge(cached: &TailRows, appended: TailRows) -> TailRows {
    let end_offset = appended.end_offset.max(cached.end_offset);
    let mut messages = cached.messages.clone();
    messages.extend(appended.messages);
    let mut record_starts = cached.record_starts.clone();
    record_starts.extend(appended.record_starts);
    let cutoff = end_offset.saturating_sub(SNAPSHOT_WINDOW_BYTES);
    let mut groups: Vec<(u64, usize)> = Vec::new();
    for start in &record_starts {
        match groups.last_mut() {
            Some(group) if group.0 == *start => group.1 += 1,
            _ => groups.push((*start, 1)),
        }
    }
    let in_window = groups
        .iter()
        .position(|group| group.0 >= cutoff)
        .unwrap_or(groups.len());
    let counts: Vec<usize> = groups[in_window..].iter().map(|group| group.1).collect();
    let first = in_window + retained_group_start(&counts, SNAPSHOT_ROW_LIMIT);
    let dropped: usize = groups[..first].iter().map(|group| group.1).sum();
    messages.drain(..dropped);
    record_starts.drain(..dropped);
    TailRows {
        start_offset: groups
            .get(first)
            .map_or(cutoff.max(cached.start_offset), |group| group.0),
        end_offset,
        messages,
        record_starts,
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TailKey {
    pub provider_id: String,
    pub path: PathBuf,
}

pub struct TailRead {
    pub rows: Arc<TailRows>,
    pub plan: TailPlan,
    pub total: u64,
    /// Transcript bytes parsed plus fingerprint bytes probed.
    pub bytes_read: u64,
}

struct Entry {
    key: TailKey,
    total: u64,
    probe: u64,
    rows: Arc<TailRows>,
    bytes: usize,
}

pub struct TailCache {
    entries: Mutex<VecDeque<Entry>>,
    max_entries: usize,
    max_bytes: usize,
}

impl Default for TailCache {
    fn default() -> Self {
        Self::with_limits(MAX_ENTRIES, MAX_BYTES)
    }
}

impl TailCache {
    pub fn with_limits(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            entries: Mutex::new(VecDeque::new()),
            max_entries,
            max_bytes,
        }
    }

    pub fn read(&self, key: &TailKey, port: &dyn TailPort) -> Option<TailRead> {
        let total = port.length()?;
        let cached = self.lookup(key);
        let mut probed = 0;
        let decided = match &cached {
            None => TailPlan::Rebuild,
            Some((cached_total, cached_probe, rows)) => {
                probed += PROBE_BYTES.min(rows.end_offset);
                let matches = port.probe(rows.end_offset) == Some(*cached_probe);
                plan(*cached_total, rows.end_offset, total, matches)
            }
        };
        let (rows, parsed) = match (decided, cached) {
            (TailPlan::Reuse, Some((_, _, rows))) => {
                return Some(TailRead {
                    rows,
                    plan: decided,
                    total,
                    bytes_read: probed,
                });
            }
            (TailPlan::Append { from }, Some((_, _, rows))) => {
                let (appended, parsed) = port.append(from, total);
                (merge(&rows, appended), parsed)
            }
            _ => port.window(),
        };
        let probe = port.probe(rows.end_offset)?;
        probed += PROBE_BYTES.min(rows.end_offset);
        let rows = Arc::new(rows);
        self.insert(key.clone(), total, probe, rows.clone());
        Some(TailRead {
            rows,
            plan: decided,
            total,
            bytes_read: parsed + probed,
        })
    }

    fn lookup(&self, key: &TailKey) -> Option<(u64, u64, Arc<TailRows>)> {
        let mut entries = self.entries.lock();
        let index = entries.iter().position(|entry| entry.key == *key)?;
        let entry = entries.remove(index)?;
        let found = (entry.total, entry.probe, entry.rows.clone());
        entries.push_back(entry);
        Some(found)
    }

    fn insert(&self, key: TailKey, total: u64, probe: u64, rows: Arc<TailRows>) {
        let bytes = charged_bytes(&key, &rows);
        let mut entries = self.entries.lock();
        entries.retain(|entry| entry.key != key);
        if bytes > self.max_bytes || self.max_entries == 0 {
            return;
        }
        entries.push_back(Entry {
            key,
            total,
            probe,
            rows,
            bytes,
        });
        while entries.len() > self.max_entries
            || entries.iter().map(|entry| entry.bytes).sum::<usize>() > self.max_bytes
        {
            entries.pop_front();
        }
    }

    pub fn evict(&self, key: &TailKey) {
        self.entries.lock().retain(|entry| entry.key != *key);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.lock().len()
    }

    /// Bounded tail messages of one session JSONL, reusing the parsed tail.
    pub fn jsonl_messages(
        &self,
        registry: &Arc<AgentRegistry>,
        provider_id: &str,
        path: &Path,
        scope: &str,
    ) -> Vec<ChatMsg> {
        let key = TailKey {
            provider_id: provider_id.to_owned(),
            path: path.to_owned(),
        };
        let port = JsonlTailPort {
            registry,
            provider_id,
            path,
        };
        let Some(read) = self.read(&key, &port) else {
            self.evict(&key);
            return Vec::new();
        };
        log_read(scope, provider_id, &read);
        read.rows.messages.clone()
    }
}

fn charged_bytes(key: &TailKey, rows: &TailRows) -> usize {
    let encoded = serde_json::to_vec(&rows.messages).map_or(0, |encoded| encoded.len());
    encoded
        .saturating_mul(2)
        .saturating_add(
            rows.messages.len() * (std::mem::size_of::<ChatMsg>() + std::mem::size_of::<u64>()),
        )
        .saturating_add(key.provider_id.len() + key.path.as_os_str().len())
        .saturating_add(std::mem::size_of::<Entry>())
}

fn log_read(scope: &str, provider_id: &str, read: &TailRead) {
    if read.plan == TailPlan::Reuse || read.rows.start_offset == 0 {
        log::debug!(
            "[chat_tail] scope={} provider={} plan={} total_bytes={} end={} bytes_read={}",
            scope,
            provider_id,
            read.plan.label(),
            read.total,
            read.rows.end_offset,
            read.bytes_read
        );
        return;
    }
    log::warn!(
        "[chat_start.bounded] scope={} provider={} plan={} total_bytes={} window_start={} window_end={} bytes_read={} messages={}",
        scope,
        provider_id,
        read.plan.label(),
        read.total,
        read.rows.start_offset,
        read.rows.end_offset,
        read.bytes_read,
        read.rows.messages.len()
    );
}

pub struct JsonlTailPort<'a> {
    pub registry: &'a Arc<AgentRegistry>,
    pub provider_id: &'a str,
    pub path: &'a Path,
}

impl TailPort for JsonlTailPort<'_> {
    fn length(&self) -> Option<u64> {
        std::fs::metadata(self.path)
            .ok()
            .map(|metadata| metadata.len())
    }

    fn probe(&self, end: u64) -> Option<u64> {
        use std::hash::{Hash, Hasher};
        let start = end.saturating_sub(PROBE_BYTES);
        let mut file = std::fs::File::open(self.path).ok()?;
        file.seek(SeekFrom::Start(start)).ok()?;
        let mut bytes = Vec::with_capacity((end - start) as usize);
        file.take(end - start).read_to_end(&mut bytes).ok()?;
        if bytes.len() as u64 != end - start {
            return None;
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut hasher);
        Some(hasher.finish())
    }

    fn window(&self) -> (TailRows, u64) {
        let snapshot = session_window::snapshot(self.registry, self.provider_id, self.path);
        let bytes_read = snapshot.bytes_read;
        (
            TailRows {
                record_starts: snapshot.row_record_starts,
                messages: snapshot.messages,
                start_offset: snapshot.start_offset,
                end_offset: snapshot.end_offset,
            },
            bytes_read,
        )
    }

    fn append(&self, from: u64, until: u64) -> (TailRows, u64) {
        let (rich, end_offset) =
            session_window::parse_window(self.registry, self.provider_id, self.path, from, until);
        let mut rows = TailRows {
            start_offset: from,
            end_offset,
            ..Default::default()
        };
        for (ordinal, row) in rich.iter().enumerate() {
            let start = row.record_start.unwrap_or(from);
            let messages = super::rich_to_chat_msgs_at(row, super::RowOrigin::at(from, ordinal));
            rows.record_starts
                .extend(std::iter::repeat_n(start, messages.len()));
            rows.messages.extend(messages);
        }
        (rows, end_offset.saturating_sub(from))
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod measure;
