use std::collections::{BTreeMap, BTreeSet, VecDeque};

use codeterm_types::blueprint_terminal::MAX_PAYLOAD_BYTES;

use super::store::Sentinel;

#[derive(Default)]
pub(super) struct Inbox {
    streams: BTreeMap<String, Stream>,
    pending: BTreeMap<String, VecDeque<String>>,
    revisions: BTreeMap<String, u64>,
    failures: BTreeSet<String>,
}

impl Inbox {
    pub fn enqueue(&mut self, id: &str, text: String) -> Result<(), String> {
        if text.is_empty() || text.len() > MAX_PAYLOAD_BYTES || text.contains('\0') {
            return Err("blueprint text must contain 1..32768 UTF-8 bytes".into());
        }
        let queue = self.pending.entry(id.into()).or_default();
        if queue.len() >= 32 {
            return Err("blueprint input queue is full (32 messages)".into());
        }
        queue.push_back(text);
        Ok(())
    }

    pub fn take(&mut self, id: &str) -> Option<String> {
        let queue = self.pending.get_mut(id)?;
        let text = queue.pop_front();
        if queue.is_empty() {
            self.pending.remove(id);
        }
        text
    }

    pub fn ready(&self) -> Vec<String> {
        self.pending.keys().cloned().collect()
    }

    pub fn retain(&mut self, jobs: &[Sentinel]) {
        let live = |id: &String| jobs.iter().any(|job| job.id == *id && !job.machine.stopped);
        self.streams.retain(|id, _| live(id));
        self.pending.retain(|id, _| live(id));
        self.revisions.retain(|id, _| live(id));
        self.failures.retain(|id| live(id));
        for job in jobs {
            self.bind(job);
        }
    }

    pub fn bind(&mut self, job: &Sentinel) {
        if self
            .revisions
            .insert(job.id.clone(), job.revision)
            .is_some_and(|revision| revision != job.revision)
        {
            self.pending.remove(&job.id);
            self.streams.remove(&job.id);
        }
    }

    pub fn failures(&mut self) -> BTreeSet<String> {
        std::mem::take(&mut self.failures)
    }

    pub fn feed(&mut self, job: &Sentinel, generation: Option<u64>, bytes: &[u8], now: u64) {
        let Some(config) = &job.terminal().cloned() else {
            return;
        };
        self.bind(job);
        let stream = self
            .streams
            .entry(job.id.clone())
            .or_insert_with(|| Stream::new(job, generation));
        if stream.generation != generation || stream.revision != job.revision {
            self.pending.remove(&job.id);
            *stream = Stream::new(job, generation);
        }
        stream.feed(bytes);
        let text = stream.text();
        if text.is_empty() || text == stream.semantic {
            return;
        }
        stream.semantic = text.clone();
        stream.deadline = config.idle_ms.map(|idle| now.saturating_add(idle));
        if stream
            .pattern
            .as_ref()
            .is_some_and(|pattern| pattern.is_match(&text))
        {
            stream.clear_delta();
            if let Err(error) = self.enqueue(&job.id, text) {
                self.failures.insert(job.id.clone());
                log::warn!("[blueprint] terminal delta refused: {error}; id={}", job.id);
            }
        }
    }

    pub fn settle(&mut self, now: u64) {
        let ready: Vec<_> = self
            .streams
            .iter_mut()
            .filter_map(|(id, stream)| {
                if !stream.deadline.is_some_and(|deadline| now >= deadline) {
                    return None;
                }
                let text = stream.text();
                stream.clear_delta();
                (!text.is_empty()).then(|| (id.clone(), text))
            })
            .collect();
        for (id, text) in ready {
            if let Err(error) = self.enqueue(&id, text) {
                self.failures.insert(id.clone());
                log::warn!("[blueprint] settled delta refused: {error}; id={id}");
            }
        }
    }

    pub fn deadline(&self) -> Option<u64> {
        self.streams
            .values()
            .filter_map(|stream| stream.deadline)
            .min()
    }
}

#[derive(Default)]
struct Stream {
    generation: Option<u64>,
    revision: u64,
    bytes: VecDeque<u8>,
    escape: u8,
    semantic: String,
    deadline: Option<u64>,
    carriage: bool,
    pattern: Option<regex::Regex>,
}

impl Stream {
    fn new(job: &Sentinel, generation: Option<u64>) -> Self {
        Self {
            generation,
            revision: job.revision,
            pattern: job
                .terminal()
                .cloned()
                .as_ref()
                .and_then(|config| config.pattern.as_ref())
                .and_then(|pattern| regex::Regex::new(pattern).ok()),
            ..Default::default()
        }
    }
    fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if self.escape == 0 && self.carriage {
                self.carriage = false;
                if byte != b'\n' {
                    while self.bytes.back().is_some_and(|byte| *byte != b'\n') {
                        self.bytes.pop_back();
                    }
                }
            }
            match (self.escape, byte) {
                (0, 0x1b) => self.escape = 1,
                (1, b'[') => self.escape = 2,
                (1, b']') => self.escape = 3,
                (1, _) => self.escape = 0,
                (2, 0x40..=0x7e) => self.escape = 0,
                (3, 0x07) | (4, b'\\') => self.escape = 0,
                (3, 0x1b) => self.escape = 4,
                (4, _) => self.escape = 3,
                (0, b'\r') => self.carriage = true,
                (0, b'\n' | b'\t' | 0x20..=0xff) => {
                    self.bytes.push_back(byte);
                    while self.bytes.len() > MAX_PAYLOAD_BYTES {
                        self.bytes.pop_front();
                    }
                }
                _ => {}
            }
        }
    }

    fn text(&self) -> String {
        let bytes: Vec<_> = self.bytes.iter().copied().collect();
        let start = bytes
            .iter()
            .position(|byte| byte & 0xc0 != 0x80)
            .unwrap_or(bytes.len());
        let bytes = &bytes[start..];
        let end = match std::str::from_utf8(bytes) {
            Err(error) if error.error_len().is_none() => error.valid_up_to(),
            _ => bytes.len(),
        };
        let mut text = normalize(&String::from_utf8_lossy(&bytes[..end]));
        let mut start = text.len().saturating_sub(MAX_PAYLOAD_BYTES);
        while !text.is_char_boundary(start) {
            start += 1;
        }
        text.drain(..start);
        text
    }

    fn clear_delta(&mut self) {
        self.bytes.clear();
        self.semantic.clear();
        self.deadline = None;
    }
}

fn normalize(text: &str) -> String {
    static TIMER: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let timer = TIMER.get_or_init(|| {
        regex::Regex::new(r"\b\d+(?:\.\d+)?(?:ms|s|m|h)\b|\b\d{1,3}:\d{2}(?::\d{2})?\b").unwrap()
    });
    let text: String = text
        .chars()
        .filter(|character| !matches!(character, '\u{2800}'..='\u{28ff}' | '◐' | '◓' | '◑' | '◒'))
        .collect();
    let text = timer.replace_all(&text, "<time>");
    text.lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn validate(job: &Sentinel) -> Result<(), String> {
    if let Some(config) = &job.terminal().cloned() {
        config.validate()?;
        if !matches!(
            job.observe().cloned(),
            Some(super::source::TabBinding::Tab(_))
        ) {
            return Err("terminal options require --observe tab:<id>".into());
        }
        if let Some(pattern) = &config.pattern {
            regex::Regex::new(pattern).map_err(|error| format!("terminal pattern: {error}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
