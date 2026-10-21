//! Durable host-side history for chatBackend tabs — otherwise the transcript
//! lives only in the plugin VM's in-memory map and is lost on reload/restart.
//! An append-only JSONL log: a message record supersedes earlier records with
//! its id, `{"_removed": id}` retires one, and the optional `{"_meta": <config>}`
//! first line lets a restored tab lazily re-open its session.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use parking_lot::Mutex;

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
    let page = codeterm_plugin::jsonl_window::page_before(path, until, budget)?;
    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::Start(page.start))?;
    let mut bytes = Vec::with_capacity((page.end - page.start) as usize);
    file.take(page.end - page.start).read_to_end(&mut bytes)?;
    let (messages, record_starts) = fold(&bytes, page.start);
    Ok(HistoryWindow {
        messages,
        record_starts,
        start_offset: page.start,
        end_offset: page.start + bytes.len() as u64,
    })
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

/// Latest version of each record in first-seen order, with each message's
/// first-record offset; retired ids drop out.
fn fold(bytes: &[u8], base: u64) -> (Vec<ChatMsg>, Vec<u64>) {
    let mut order: Vec<(String, u64)> = Vec::new();
    let mut by_id: HashMap<String, ChatMsg> = HashMap::new();
    let mut offset = base;
    for raw in bytes.split_inclusive(|byte| *byte == b'\n') {
        let start = offset;
        offset += raw.len() as u64;
        let Ok(line) = std::str::from_utf8(raw) else {
            continue;
        };
        let line = line.trim();
        if line.is_empty() || line.starts_with(META_PREFIX) {
            continue;
        }
        if line.starts_with(REMOVED_PREFIX) {
            if let Some(id) = removed_id(line) {
                by_id.remove(&id);
                order.retain(|(kept, _)| *kept != id);
            }
            continue;
        }
        let Ok(message) = serde_json::from_str::<ChatMsg>(line) else {
            continue;
        };
        if !by_id.contains_key(&message.id) {
            order.push((message.id.clone(), start));
        }
        by_id.insert(message.id.clone(), message);
    }
    order
        .into_iter()
        .filter_map(|(id, start)| Some((by_id.remove(&id)?.with_segments(), start)))
        .unzip()
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

/// Registry of the single shared writer per `(plugin_id, session_id)`. Lives for
/// the daemon's lifetime; the per-session footprint is one digest per message id.
static SHARED: LazyLock<Mutex<HashMap<(String, String), SharedHistory>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

struct Live {
    digest: u64,
}

/// Single writer of one session log. Holds only id digests, so an unchanged
/// upsert is a no-op and a changed one appends one record (or rewrites the
/// final record in place when it is the same message, as streaming does).
pub struct ChatBackendHistory {
    path: PathBuf,
    meta: bool,
    live: HashMap<String, Live>,
    /// Id and start of the final record when it is a message record.
    last: Option<(String, u64)>,
    /// End of the last complete record; bytes past it are a torn write.
    length: u64,
}

fn digest(line: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    line.hash(&mut hasher);
    hasher.finish()
}

impl ChatBackendHistory {
    pub fn open(plugin_id: &str, session_id: &str) -> Self {
        Self::open_at(session_path(plugin_id, session_id))
    }

    pub fn open_at(path: PathBuf) -> Self {
        let mut history = Self {
            path,
            meta: false,
            live: HashMap::new(),
            last: None,
            length: 0,
        };
        if let Err(error) = history.index() {
            if error.kind() != std::io::ErrorKind::NotFound {
                log::warn!("[chatbackend] transcript index failed: {error}");
            }
        }
        history
    }

    fn index(&mut self) -> std::io::Result<()> {
        let file = std::fs::File::open(&self.path)?;
        let total = file.metadata()?.len();
        let mut reader = BufReader::new(file);
        let mut line = Vec::new();
        let mut offset = 0;
        loop {
            line.clear();
            let count = reader.read_until(b'\n', &mut line)?;
            if count == 0 || line.last() != Some(&b'\n') {
                break;
            }
            self.observe(
                std::str::from_utf8(&line).unwrap_or_default().trim(),
                offset,
            );
            offset += count as u64;
        }
        self.length = offset;
        if total > offset {
            self.preserve_torn_tail(offset);
        }
        Ok(())
    }

    fn observe(&mut self, line: &str, offset: u64) {
        self.last = None;
        if line.starts_with(META_PREFIX) {
            self.meta = true;
        } else if line.starts_with(REMOVED_PREFIX) {
            if let Some(id) = removed_id(line) {
                self.live.remove(&id);
            }
        } else if let Ok(head) = serde_json::from_str::<RecordId>(line) {
            self.live.insert(
                head.id.clone(),
                Live {
                    digest: digest(line),
                },
            );
            self.last = Some((head.id, offset));
        }
    }

    fn preserve_torn_tail(&self, complete: u64) {
        let copy = || -> std::io::Result<()> {
            let mut file = std::fs::File::open(&self.path)?;
            file.seek(SeekFrom::Start(complete))?;
            let mut torn = Vec::new();
            file.read_to_end(&mut torn)?;
            let name = self.path.file_name().unwrap_or_default().to_string_lossy();
            let millis = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_millis());
            std::fs::write(
                self.path
                    .with_file_name(format!("{name}.corrupt-{millis:013}")),
                torn,
            )
        };
        if let Err(error) = copy() {
            log::warn!("[chatbackend] torn transcript tail not preserved: {error}");
        }
    }

    /// The single shared writer for `(plugin_id, session_id)`. Both the poll-loop
    /// sink and the host-side gateway upsert through this instance, so a write
    /// can't clobber a row it didn't author (single-writer invariant).
    pub fn shared(plugin_id: &str, session_id: &str) -> SharedHistory {
        let key = (plugin_id.to_string(), session_id.to_string());
        if let Some(h) = SHARED.lock().get(&key) {
            return h.clone();
        }
        // Build outside the registry lock (never hold it across disk IO); insert is idempotent, first writer wins.
        let built: SharedHistory = Arc::new(Mutex::new(Self::open(plugin_id, session_id)));
        SHARED.lock().entry(key).or_insert(built).clone()
    }

    /// Seed the openSession config as the `_meta` first line if not already present.
    pub fn ensure_meta(&mut self, config: &serde_json::Value) {
        if self.meta || config.is_null() {
            return;
        }
        let Ok(line) = serde_json::to_string(&serde_json::json!({ META_KEY: config })) else {
            return;
        };
        let result = if self.length == 0 {
            self.write_at(0, &line)
        } else {
            self.prepend(&line)
        };
        match result {
            Ok(()) => self.meta = true,
            Err(error) => log::warn!("[chatbackend] transcript meta not saved: {error}"),
        }
    }

    /// UPSERT messages by id. Returns whether anything changed (and was written).
    pub fn upsert(&mut self, messages: &[ChatMsg]) -> bool {
        let mut changed = false;
        for message in messages {
            let Ok(line) = serde_json::to_string(message) else {
                continue;
            };
            let digest = digest(&line);
            if self
                .live
                .get(&message.id)
                .is_some_and(|live| live.digest == digest)
            {
                continue;
            }
            let at = match &self.last {
                Some((id, start)) if *id == message.id => *start,
                _ => self.length,
            };
            if let Err(error) = self.write_at(at, &line) {
                log::warn!("[chatbackend] transcript not saved: {error}");
                continue;
            }
            self.live.insert(message.id.clone(), Live { digest });
            self.last = Some((message.id.clone(), at));
            changed = true;
        }
        changed
    }

    pub fn remove(&mut self, message_id: &str) -> bool {
        if self.live.remove(message_id).is_none() {
            return false;
        }
        let result = match self.last.take() {
            Some((id, start)) if id == message_id => self.truncate(start),
            _ => serde_json::to_string(&serde_json::json!({ REMOVED_KEY: message_id }))
                .map_err(std::io::Error::other)
                .and_then(|line| {
                    let end = self.length;
                    self.write_at(end, &line)
                }),
        };
        if let Err(error) = result {
            log::warn!("[chatbackend] transcript removal not saved: {error}");
        }
        true
    }

    fn open_for_write(&self) -> std::io::Result<std::fs::File> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&self.path)
    }

    fn write_at(&mut self, start: u64, line: &str) -> std::io::Result<()> {
        let mut file = self.open_for_write()?;
        file.seek(SeekFrom::Start(start))?;
        let mut record = Vec::with_capacity(line.len() + 1);
        record.extend_from_slice(line.as_bytes());
        record.push(b'\n');
        file.write_all(&record)?;
        let end = start + record.len() as u64;
        if file.metadata()?.len() != end {
            file.set_len(end)?;
        }
        self.length = end;
        Ok(())
    }

    fn truncate(&mut self, start: u64) -> std::io::Result<()> {
        self.open_for_write()?.set_len(start)?;
        self.length = start;
        Ok(())
    }

    /// One-time rewrite that puts `line` first when the log already has records.
    fn prepend(&mut self, line: &str) -> std::io::Result<()> {
        let staged = self.path.with_extension("jsonl.meta-staging");
        {
            let mut out = std::io::BufWriter::new(std::fs::File::create(&staged)?);
            out.write_all(line.as_bytes())?;
            out.write_all(b"\n")?;
            let source = std::fs::File::open(&self.path)?;
            std::io::copy(&mut source.take(self.length), &mut out)?;
            out.flush()?;
        }
        std::fs::rename(&staged, &self.path)?;
        let shift = line.len() as u64 + 1;
        self.length += shift;
        if let Some((_, start)) = self.last.as_mut() {
            *start += shift;
        }
        Ok(())
    }
}

#[derive(serde::Deserialize)]
struct RecordId {
    id: String,
}

#[cfg(test)]
mod tests;
