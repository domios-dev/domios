use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use super::{versions, ChatMsg, Operation, META_KEY, META_PREFIX, REMOVED_KEY, REMOVED_PREFIX};

pub(super) struct HistoryLog {
    path: PathBuf,
    indexed: bool,
    meta: Option<serde_json::Value>,
    live: HashMap<String, Live>,
    last: Option<(String, u64, String)>,
    length: u64,
    pending_sync: bool,
}

#[derive(Clone, Copy)]
struct Live {
    offset: u64,
    delta_bytes: usize,
}

impl HistoryLog {
    pub(super) fn memory_stats(&self) -> (usize, usize) {
        let slots = self.live.capacity() * (std::mem::size_of::<(String, Live)>() + 64);
        let last = self
            .last
            .as_ref()
            .map_or(0, |(id, _, encoded)| id.capacity() + encoded.capacity());
        (self.live.len(), slots + last)
    }

    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            indexed: false,
            meta: None,
            live: HashMap::new(),
            last: None,
            length: 0,
            pending_sync: false,
        }
    }

    pub fn apply(&mut self, operation: Operation) -> io::Result<bool> {
        self.ensure_indexed()?;
        let result = match operation {
            Operation::Meta(config) => self.ensure_meta(config),
            Operation::Upsert(messages) => self.upsert(messages),
            Operation::Remove(id) => self.remove(&id),
            Operation::Compact => self.compact().map(|()| true),
            Operation::Snapshot(rows) => {
                if self.path.is_file() {
                    let previous = super::try_read_messages(&self.path)?;
                    if previous.last().map(|row| row.seq) > rows.last().map(|row| row.seq) {
                        return Ok(false);
                    }
                }
                let mut bytes = Vec::new();
                for row in rows {
                    serde_json::to_writer(&mut bytes, &row).map_err(io::Error::other)?;
                    bytes.push(b'\n');
                }
                if bytes.len() > super::TAIL_BYTES as usize {
                    return Err(io::Error::other(
                        "Runtime transcript exceeds history budget",
                    ));
                }
                codeterm_util::atomic_write::write_atomic(&self.path, bytes)?;
                self.indexed = false;
                self.last = None;
                self.ensure_indexed()?;
                Ok(true)
            }
        };
        if result.is_err() {
            self.indexed = false;
            self.last = None;
        }
        result
    }

    fn ensure_indexed(&mut self) -> io::Result<()> {
        if self.indexed {
            return Ok(());
        }
        self.live.clear();
        self.meta = None;
        self.length = 0;
        let file = match std::fs::File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.indexed = true;
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        let total = file.metadata()?.len();
        let mut reader = BufReader::new(file);
        let mut line = Vec::new();
        loop {
            line.clear();
            let count = reader.read_until(b'\n', &mut line)?;
            if count == 0 || line.last() != Some(&b'\n') {
                break;
            }
            let line = std::str::from_utf8(&line).map_err(io::Error::other)?;
            if line.starts_with(META_PREFIX) {
                self.meta = serde_json::from_str::<serde_json::Value>(line)
                    .map_err(io::Error::other)?
                    .get(META_KEY)
                    .cloned();
            } else if line.starts_with(REMOVED_PREFIX) {
                if let Some(id) = super::removed_id(line) {
                    self.live.remove(&id);
                }
            } else if !line.trim().is_empty() {
                let id = versions::message_id(line.trim())?;
                let delta_bytes = if let Some(base) = versions::base(line.trim())? {
                    let previous = self
                        .live
                        .get(&id)
                        .filter(|live| live.offset == base)
                        .ok_or_else(|| io::Error::other("history version base is not current"))?;
                    previous.delta_bytes.saturating_add(count)
                } else {
                    0
                };
                self.live.insert(
                    id,
                    Live {
                        offset: self.length,
                        delta_bytes,
                    },
                );
            }
            self.length += count as u64;
        }
        drop(reader);
        if total > self.length {
            self.preserve_torn_tail()?;
        }
        if self.pending_sync {
            std::fs::OpenOptions::new()
                .write(true)
                .open(&self.path)?
                .sync_data()?;
            self.pending_sync = false;
        }
        self.indexed = true;
        Ok(())
    }

    fn preserve_torn_tail(&self) -> io::Result<()> {
        let mut file = std::fs::File::open(&self.path)?;
        file.seek(SeekFrom::Start(self.length))?;
        let mut torn = Vec::new();
        file.read_to_end(&mut torn)?;
        let name = self.path.file_name().unwrap_or_default().to_string_lossy();
        let unique = uuid::Uuid::new_v4();
        std::fs::write(
            self.path.with_file_name(format!("{name}.corrupt-{unique}")),
            torn,
        )
    }

    fn append(&mut self, line: &str) -> io::Result<u64> {
        let at = self.length;
        let mut bytes = Vec::with_capacity(line.len() + 1);
        bytes.extend_from_slice(line.as_bytes());
        bytes.push(b'\n');
        self.flush(&mut bytes)?;
        Ok(at)
    }

    /// One write and one sync for however many records are waiting.
    fn flush(&mut self, batch: &mut Vec<u8>) -> io::Result<()> {
        if batch.is_empty() {
            return Ok(());
        }
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&self.path)?;
        if file.metadata()?.len() != self.length {
            file.set_len(self.length)?;
        }
        file.seek(SeekFrom::Start(self.length))?;
        self.pending_sync = true;
        file.write_all(batch)?;
        file.sync_data()?;
        self.pending_sync = false;
        self.length += batch.len() as u64;
        batch.clear();
        Ok(())
    }

    fn upsert(&mut self, messages: Vec<ChatMsg>) -> io::Result<bool> {
        let mut changed = false;
        let mut batch = Vec::new();
        for message in messages {
            let full =
                serde_json::to_string(&serde_json::to_value(&message).map_err(io::Error::other)?)
                    .map_err(io::Error::other)?;
            let previous = match self.live.get(&message.id).copied() {
                Some(live) => {
                    let at = live.offset;
                    let cached = match &self.last {
                        Some((id, cached_at, line)) if *id == message.id && *cached_at == at => {
                            Some(line.clone())
                        }
                        _ => None,
                    };
                    let before = match cached {
                        Some(line) => line,
                        None => {
                            // A version still in the batch has to reach the file to be read.
                            if at >= self.length {
                                self.flush(&mut batch)?;
                            }
                            versions::resolve(
                                &mut std::fs::File::open(&self.path)?,
                                at,
                                &message.id,
                            )?
                        }
                    };
                    Some((live, before))
                }
                None => None,
            };
            let (line, delta_bytes) = if let Some((live, before)) = previous {
                if full == before {
                    continue;
                }
                let delta = versions::delta(&message.id, live.offset, &before, &full)?;
                let cost = live.delta_bytes.saturating_add(delta.len() + 1);
                if cost >= full.len() {
                    (full.clone(), 0)
                } else {
                    (delta, cost)
                }
            } else {
                (full.clone(), 0)
            };
            let at = self.length + batch.len() as u64;
            batch.extend_from_slice(line.as_bytes());
            batch.push(b'\n');
            self.live.insert(
                message.id.clone(),
                Live {
                    offset: at,
                    delta_bytes,
                },
            );
            self.last = Some((message.id, at, full));
            changed = true;
        }
        self.flush(&mut batch)?;
        Ok(changed)
    }

    fn remove(&mut self, id: &str) -> io::Result<bool> {
        if !self.live.contains_key(id) {
            return Ok(false);
        }
        let line = serde_json::to_string(&serde_json::json!({ REMOVED_KEY: id }))
            .map_err(io::Error::other)?;
        self.append(&line)?;
        self.live.remove(id);
        self.last = None;
        Ok(true)
    }

    fn ensure_meta(&mut self, config: serde_json::Value) -> io::Result<bool> {
        if self.meta.is_some() || config.is_null() {
            return Ok(false);
        }
        if self.length == 0 {
            let line = serde_json::to_string(&serde_json::json!({ META_KEY: config }))
                .map_err(io::Error::other)?;
            self.append(&line)?;
            self.meta = Some(config);
        } else {
            self.replace(Some(config))?;
        }
        Ok(true)
    }

    fn compact(&mut self) -> io::Result<()> {
        self.replace(self.meta.clone())
    }

    fn replace(&mut self, meta: Option<serde_json::Value>) -> io::Result<()> {
        let mut bytes = Vec::new();
        if let Some(meta) = &meta {
            serde_json::to_writer(&mut bytes, &serde_json::json!({ META_KEY: meta }))
                .map_err(io::Error::other)?;
            bytes.push(b'\n');
        }
        if self.length > 0 {
            let mut file = std::fs::File::open(&self.path)?;
            let mut source = Vec::new();
            (&mut file).take(self.length).read_to_end(&mut source)?;
            let (messages, _) = versions::fold(&mut file, &source, 0)?;
            for message in messages {
                serde_json::to_writer(&mut bytes, &message).map_err(io::Error::other)?;
                bytes.push(b'\n');
            }
        }
        codeterm_util::atomic_write::write_atomic(&self.path, bytes)?;
        self.indexed = false;
        self.last = None;
        self.ensure_indexed()
    }
}
