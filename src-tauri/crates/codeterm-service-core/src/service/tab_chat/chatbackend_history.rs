//! Durable host-side history for chatBackend tabs — otherwise the transcript
//! lives only in the plugin VM's in-memory map and is lost on reload/restart.
//! An append-only JSONL log: a message record supersedes earlier records with
//! its id, `{"_removed": id}` retires one, and the optional `{"_meta": <config>}`
//! first line lets a restored tab lazily re-open its session.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Weak};

use parking_lot::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

pub fn memory_stats() -> serde_json::Value {
    let Some(entries) = SHARED.try_lock() else {
        return serde_json::json!({"busy": true});
    };
    let mut bytes = 0;
    let mut live = 0;
    let mut busy = 0;
    let mut indexed_messages = 0;
    for entry in entries.values() {
        if let Some(writer) = entry.writer.upgrade() {
            live += 1;
            if let Some(writer) = writer.try_lock() {
                bytes += writer.bytes.load(Ordering::Relaxed);
                indexed_messages += writer.indexed_messages.load(Ordering::Relaxed);
            } else {
                busy += 1;
            }
        }
    }
    serde_json::json!({
        "registry_entries": entries.len(), "live_writers": live,
        "busy_writers": busy, "approx_bytes": bytes, "indexed_messages": indexed_messages,
        "estimate": "index slots with 64 bytes per message id, plus last encoded message",
    })
}

use crate::agent::chat_types::ChatMsg;

const SESSIONS_SUBDIR: &str = "chatbackend-sessions";
const META_KEY: &str = "_meta";
const REMOVED_KEY: &str = "_removed";
const META_PREFIX: &str = "{\"_meta\":";
const REMOVED_PREFIX: &str = "{\"_removed\":";
const MAX_META_BYTES: u64 = 1024 * 1024;
/// Trailing bytes a snapshot or context read folds; older records page on demand.
pub const TAIL_BYTES: u64 = 4 * 1024 * 1024;
/// Soft byte limit for one older-history page.
pub const PAGE_BYTES: u64 = 1024 * 1024;

/// Filesystem-safe path component: keep `[A-Za-z0-9._-]`, map anything else to
/// `_` so a plugin-chosen id with slashes/colons can't escape the sessions dir.
fn sanitize(component: &str) -> String {
    component
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// `~/.domios[-dev]/chatbackend-sessions/<plugin_id>/`.
pub fn session_dir(plugin_id: &str) -> PathBuf {
    crate::paths::data_dir()
        .join(SESSIONS_SUBDIR)
        .join(sanitize(plugin_id))
}

/// JSONL path for one session.
pub fn session_path(plugin_id: &str, session_id: &str) -> PathBuf {
    session_dir(plugin_id).join(format!("{}.jsonl", sanitize(session_id)))
}

fn session_digest(plugin_id: &str, session_id: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(plugin_id.as_bytes());
    hash.update([0]);
    hash.update(session_id.as_bytes());
    format!("{:x}", hash.finalize())
}

pub fn runtime_path(plugin_id: &str, session_id: &str) -> PathBuf {
    session_dir(plugin_id)
        .join("runtime")
        .join(format!("{}.jsonl", session_digest(plugin_id, session_id)))
}

/// Append-only log of every row a runtime session produced. The runtime snapshot
/// holds only the recent rows a restart restores; a closed tab reads this one.
pub fn archive_path(plugin_id: &str, session_id: &str) -> PathBuf {
    session_dir(plugin_id)
        .join("archive")
        .join(format!("{}.jsonl", session_digest(plugin_id, session_id)))
}

/// The older half of a session log that outgrew [`ARCHIVE_ROTATE_BYTES`].
pub fn archive_rotated_path(plugin_id: &str, session_id: &str) -> PathBuf {
    session_dir(plugin_id).join("archive").join(format!(
        "{}.prev.jsonl",
        session_digest(plugin_id, session_id)
    ))
}

/// Past this size a session log rotates once, bounding one session to about twice it.
pub const ARCHIVE_ROTATE_BYTES: u64 = 32 * 1024 * 1024;

/// Sidecar path for the host-composed context decision record. Shares
/// `session_dir` and `sanitize`, so a plugin-chosen id still cannot escape it.
pub fn context_sidecar_path(plugin_id: &str, session_id: &str) -> PathBuf {
    session_dir(plugin_id).join(format!("{}.context.jsonl", sanitize(session_id)))
}

/// Complete records of one byte range, folded to their latest versions.
#[derive(Debug, Default)]
pub struct HistoryWindow {
    pub messages: Vec<ChatMsg>,
    /// First-record offset of each message, parallel to `messages`.
    pub record_starts: Vec<u64>,
    pub start_offset: u64,
    pub end_offset: u64,
}

impl HistoryWindow {
    pub fn truncated(&self) -> bool {
        self.start_offset > 0
    }

    /// Keep the newest `limit` messages; the window then starts at the first kept record.
    pub fn keep_last(mut self, limit: usize) -> Self {
        let excess = self.messages.len().saturating_sub(limit);
        if excess > 0 {
            self.messages.drain(..excess);
            self.record_starts.drain(..excess);
            self.start_offset = self
                .record_starts
                .first()
                .copied()
                .unwrap_or(self.end_offset);
        }
        self
    }
}

/// The trailing [`TAIL_BYTES`] of the transcript. Missing file ⇒ `NotFound`.
pub fn read_tail(path: &Path) -> std::io::Result<HistoryWindow> {
    let length = std::fs::metadata(path)?.len();
    read_window(path, length, TAIL_BYTES)
}

/// One complete-record window ending at or below `until`.
pub fn read_window(path: &Path, until: u64, budget: u64) -> std::io::Result<HistoryWindow> {
    let mut file = std::fs::File::open(path)?;
    let page = codeterm_plugin::jsonl_window::page_before_file(&mut file, until, budget)?;
    file.seek(SeekFrom::Start(page.start))?;
    let mut bytes = Vec::with_capacity((page.end - page.start) as usize);
    (&mut file)
        .take(page.end - page.start)
        .read_to_end(&mut bytes)?;
    let (messages, record_starts) = versions::fold(&mut file, &bytes, page.start)?;
    Ok(HistoryWindow {
        messages,
        record_starts,
        start_offset: page.start,
        end_offset: page.start + bytes.len() as u64,
    })
}

/// Every message of a bounded session log; an explicit closed-tab read, never a hot path.
pub fn read_log(path: &Path) -> std::io::Result<Vec<ChatMsg>> {
    let mut file = std::fs::File::open(path)?;
    let length = file.metadata()?.len();
    let limit = 2 * ARCHIVE_ROTATE_BYTES;
    if length > limit {
        return read_window(path, length, limit).map(|window| window.messages);
    }
    let mut bytes = Vec::with_capacity(length as usize);
    (&mut file).read_to_end(&mut bytes)?;
    versions::fold(&mut file, &bytes, 0).map(|(messages, _)| messages)
}

/// Messages of the trailing window; missing or unreadable ⇒ empty.
pub fn read_messages(path: &Path) -> Vec<ChatMsg> {
    try_read_messages(path).unwrap_or_default()
}

/// Like [`read_messages`], but keeps the read error so a caller that must not
/// degrade silently can tell "no transcript yet" from "transcript unreadable".
pub fn try_read_messages(path: &Path) -> std::io::Result<Vec<ChatMsg>> {
    read_tail(path).map(|window| window.messages)
}

fn removed_id(line: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()?
        .get(REMOVED_KEY)?
        .as_str()
        .map(str::to_owned)
}

/// The openSession config from the `_meta` first line.
pub fn read_meta(path: &Path) -> Option<serde_json::Value> {
    let file = std::fs::File::open(path).ok()?;
    let mut first = Vec::new();
    BufReader::new(file.take(MAX_META_BYTES))
        .read_until(b'\n', &mut first)
        .ok()?;
    let line = std::str::from_utf8(&first).ok()?.trim();
    if !line.starts_with(META_PREFIX) {
        return None;
    }
    serde_json::from_str::<serde_json::Value>(line)
        .ok()?
        .get(META_KEY)
        .cloned()
}

/// Move a session's JSONL to a new id (best-effort), so a re-opened session keeps
/// continuous history. No-op if ids match, source is missing, or dest exists.
pub fn migrate(plugin_id: &str, from_session: &str, to_session: &str) {
    if from_session == to_session {
        return;
    }
    let from = session_path(plugin_id, from_session);
    let to = session_path(plugin_id, to_session);
    if from.exists() && !to.exists() {
        if let Some(parent) = to.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::rename(&from, &to);
    }
}

/// A process-shared, lock-guarded history writer. See [`ChatBackendHistory::shared`].
pub type SharedHistory = Arc<Mutex<ChatBackendHistory>>;

struct RegisteredHistory {
    writer: Weak<Mutex<ChatBackendHistory>>,
    live: Option<SharedHistory>,
}

type HistoryRegistry = HashMap<(String, String), RegisteredHistory>;

static SHARED: LazyLock<Mutex<HistoryRegistry>> = LazyLock::new(|| Mutex::new(HashMap::new()));

mod storage;
mod versions;

pub struct ChatBackendHistory {
    path: PathBuf,
    owner: Option<std::sync::mpsc::SyncSender<Request>>,
    bytes: Arc<AtomicUsize>,
    indexed_messages: Arc<AtomicUsize>,
}

enum Operation {
    Meta(serde_json::Value),
    Upsert(Vec<ChatMsg>),
    Remove(String),
    Compact,
    Snapshot(Vec<ChatMsg>),
}

struct Request {
    operation: Operation,
    reply: std::sync::mpsc::SyncSender<std::io::Result<bool>>,
}

impl ChatBackendHistory {
    pub fn open(plugin_id: &str, session_id: &str) -> Self {
        Self::open_at(session_path(plugin_id, session_id))
    }

    pub fn open_at(path: PathBuf) -> Self {
        Self::unindexed(path)
    }

    fn unindexed(path: PathBuf) -> Self {
        Self {
            path,
            owner: None,
            bytes: Arc::new(AtomicUsize::new(0)),
            indexed_messages: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn shared(plugin_id: &str, session_id: &str) -> SharedHistory {
        shared_in(
            &SHARED,
            (plugin_id.into(), session_id.into()),
            session_path(plugin_id, session_id),
        )
    }

    pub fn runtime_shared(plugin_id: &str, session_id: &str) -> SharedHistory {
        shared_in(
            &SHARED,
            (format!("runtime:{plugin_id}"), session_id.into()),
            runtime_path(plugin_id, session_id),
        )
    }

    pub fn snapshot(&mut self, rows: Vec<ChatMsg>) -> std::io::Result<()> {
        self.request(Operation::Snapshot(rows)).map(|_| ())
    }

    pub fn release(plugin_id: &str, session_id: &str) {
        release_in(&mut SHARED.lock(), &(plugin_id.into(), session_id.into()));
    }

    fn request(&mut self, operation: Operation) -> std::io::Result<bool> {
        if self.owner.is_none() {
            let (sender, receiver) = std::sync::mpsc::sync_channel::<Request>(1);
            let path = self.path.clone();
            let bytes = self.bytes.clone();
            let indexed_messages = self.indexed_messages.clone();
            std::thread::Builder::new()
                .name("chat-history".into())
                .spawn(move || {
                    let mut log = storage::HistoryLog::new(path);
                    while let Ok(request) = receiver.recv() {
                        let result = log.apply(request.operation);
                        let (count, size) = log.memory_stats();
                        bytes.store(size, Ordering::Relaxed);
                        indexed_messages.store(count, Ordering::Relaxed);
                        let _ = request.reply.send(result);
                    }
                })?;
            self.owner = Some(sender);
        }
        let (reply, result) = std::sync::mpsc::sync_channel(1);
        self.owner
            .as_ref()
            .ok_or_else(|| std::io::Error::other("history owner unavailable"))?
            .send(Request { operation, reply })
            .map_err(|_| std::io::Error::other("history owner stopped"))?;
        result
            .recv()
            .map_err(|_| std::io::Error::other("history owner stopped"))?
    }

    pub fn ensure_meta(&mut self, config: &serde_json::Value) {
        if let Err(error) = self.request(Operation::Meta(config.clone())) {
            log::warn!("[chatbackend] transcript meta not saved: {error}");
        }
    }

    pub fn try_upsert(&mut self, messages: &[ChatMsg]) -> std::io::Result<bool> {
        self.request(Operation::Upsert(messages.to_vec()))
    }

    pub fn upsert(&mut self, messages: &[ChatMsg]) -> bool {
        match self.try_upsert(messages) {
            Ok(changed) => changed,
            Err(error) => {
                log::warn!("[chatbackend] transcript not saved: {error}");
                false
            }
        }
    }

    pub fn remove(&mut self, id: &str) -> bool {
        match self.request(Operation::Remove(id.into())) {
            Ok(changed) => changed,
            Err(error) => {
                log::warn!("[chatbackend] transcript removal not saved: {error}");
                false
            }
        }
    }

    pub fn compact(&mut self) -> std::io::Result<()> {
        self.request(Operation::Compact).map(|_| ())
    }
}

fn shared_in(
    registry: &Mutex<HistoryRegistry>,
    key: (String, String),
    path: PathBuf,
) -> SharedHistory {
    let mut entries = registry.lock();
    entries.retain(|_, entry| entry.writer.strong_count() > 0);
    if let Some(writer) = entries.get(&key).and_then(|entry| entry.writer.upgrade()) {
        return writer;
    }
    let built = Arc::new(Mutex::new(ChatBackendHistory::unindexed(path)));
    entries.insert(
        key,
        RegisteredHistory {
            writer: Arc::downgrade(&built),
            live: Some(built.clone()),
        },
    );
    built
}

fn release_in(registry: &mut HistoryRegistry, key: &(String, String)) {
    // In-flight sinks must remain discoverable until their last handle is dropped.
    if let Some(entry) = registry.get_mut(key) {
        entry.live = None;
        if entry.writer.strong_count() == 0 {
            registry.remove(key);
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod version_tests;
