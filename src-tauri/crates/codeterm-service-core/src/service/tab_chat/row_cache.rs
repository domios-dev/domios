use std::collections::VecDeque;
use std::sync::Arc;

use parking_lot::Mutex;

use super::results::{ChatStartKind, KnownChatWindow};
use super::resume_cursor::ChatEpoch;
use super::session_window::{attach_kind, SessionSnapshot};

const MAX_ENTRIES: usize = 16;
const MAX_BYTES: usize = 32 * 1024 * 1024;

pub struct CacheAttach {
    pub kind: ChatStartKind,
    pub snapshot: SessionSnapshot,
    pub cache_hit: bool,
    pub cache_refresh: bool,
}

pub struct CachedRows {
    tab_id: String,
    known: KnownChatWindow,
    snapshot: SessionSnapshot,
    bytes: usize,
}

impl CachedRows {
    pub fn file_length(&self) -> u64 {
        self.known.file_length
    }

    pub fn resume(
        &self,
        epoch: &ChatEpoch,
        incarnation: &str,
        total: u64,
        read_append: impl FnOnce(&KnownChatWindow) -> Option<SessionSnapshot>,
    ) -> Option<SessionSnapshot> {
        match attach_kind(&self.known, epoch, incarnation, total) {
            ChatStartKind::Snapshot => None,
            ChatStartKind::Unchanged => {
                let mut snapshot = self.snapshot.clone();
                snapshot.bytes_read = 0;
                Some(snapshot)
            }
            ChatStartKind::Delta => self.extend(read_append(&self.known)?),
        }
    }

    fn extend(&self, mut append: SessionSnapshot) -> Option<SessionSnapshot> {
        if append.start_offset != self.known.end_offset {
            return None;
        }
        let mut messages = self.snapshot.messages.clone();
        messages.append(&mut append.messages);
        append.messages = messages;
        append.start_offset = self.snapshot.start_offset;
        if append.context_usage.is_none() {
            append.context_usage = self.snapshot.context_usage.clone();
        }
        Some(append)
    }
}

pub struct RowCache {
    entries: Mutex<VecDeque<Arc<CachedRows>>>,
    max_entries: usize,
    max_bytes: usize,
}

impl Default for RowCache {
    fn default() -> Self {
        Self::with_limits(MAX_ENTRIES, MAX_BYTES)
    }
}

impl RowCache {
    pub fn attach(
        &self,
        tab_id: &str,
        epoch: &ChatEpoch,
        incarnation: &str,
        total: u64,
        known: Option<&KnownChatWindow>,
        read: impl Fn(&KnownChatWindow) -> Option<(ChatStartKind, SessionSnapshot)>,
    ) -> Option<CacheAttach> {
        if let Some(known) = known {
            match attach_kind(known, epoch, incarnation, total) {
                ChatStartKind::Unchanged => {
                    return Some(CacheAttach {
                        kind: ChatStartKind::Unchanged,
                        snapshot: SessionSnapshot {
                            total,
                            start_offset: known.end_offset,
                            end_offset: known.end_offset,
                            ..Default::default()
                        },
                        cache_hit: false,
                        cache_refresh: false,
                    })
                }
                ChatStartKind::Delta => {
                    if let Some((kind, snapshot)) = read(known) {
                        return Some(CacheAttach {
                            kind,
                            snapshot,
                            cache_hit: false,
                            cache_refresh: false,
                        });
                    }
                }
                ChatStartKind::Snapshot => {}
            }
        }
        let entry = self.get(tab_id, epoch, incarnation, total)?;
        let cache_refresh = entry.file_length() != total;
        let snapshot = entry.resume(epoch, incarnation, total, |known| {
            read(known).map(|(_, snapshot)| snapshot)
        })?;
        Some(CacheAttach {
            kind: ChatStartKind::Snapshot,
            snapshot,
            cache_hit: true,
            cache_refresh,
        })
    }

    pub fn extend_delta(
        &self,
        tab_id: &str,
        epoch: &ChatEpoch,
        incarnation: &str,
        cursor: String,
        delta: &SessionSnapshot,
    ) {
        if let Some(entry) = self.get(tab_id, epoch, incarnation, delta.total) {
            if let Some(snapshot) = entry.extend(delta.clone()) {
                self.insert(tab_id, epoch, cursor, &snapshot);
            }
        }
    }

    fn with_limits(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            entries: Mutex::new(VecDeque::new()),
            max_entries,
            max_bytes,
        }
    }

    pub fn get(
        &self,
        tab_id: &str,
        epoch: &ChatEpoch,
        incarnation: &str,
        total: u64,
    ) -> Option<Arc<CachedRows>> {
        let mut entries = self.entries.lock();
        let index = entries.iter().position(|entry| entry.tab_id == tab_id)?;
        let entry = entries.remove(index)?;
        if attach_kind(&entry.known, epoch, incarnation, total) == ChatStartKind::Snapshot {
            return None;
        }
        entries.push_back(entry.clone());
        Some(entry)
    }

    pub fn insert(
        &self,
        tab_id: &str,
        epoch: &ChatEpoch,
        cursor: String,
        snapshot: &SessionSnapshot,
    ) {
        let Ok(encoded) = serde_json::to_vec(&snapshot.messages) else {
            return;
        };
        let bytes = encoded
            .len()
            .saturating_mul(4)
            .saturating_add(
                snapshot.messages.len() * std::mem::size_of::<crate::agent::chat_types::ChatMsg>(),
            )
            .saturating_add(
                cursor.len()
                    + tab_id.len()
                    + epoch.source_session_id.len()
                    + epoch.session_binding_nonce.as_ref().map_or(0, String::len),
            )
            .saturating_add(std::mem::size_of::<CachedRows>());
        drop(encoded);
        let retained = SessionSnapshot {
            messages: snapshot.messages.clone(),
            context_usage: snapshot.context_usage.clone(),
            total: snapshot.total,
            start_offset: snapshot.start_offset,
            end_offset: snapshot.end_offset,
            ..Default::default()
        };
        let mut entries = self.entries.lock();
        entries.retain(|entry| entry.tab_id != tab_id);
        if bytes > self.max_bytes || self.max_entries == 0 {
            return;
        }
        entries.push_back(Arc::new(CachedRows {
            tab_id: tab_id.into(),
            known: KnownChatWindow {
                source_session_id: epoch.source_session_id.clone(),
                session_binding_nonce: epoch.session_binding_nonce.clone(),
                end_offset: snapshot.end_offset,
                file_length: snapshot.total,
                cursor,
            },
            snapshot: retained,
            bytes,
        }));
        while entries.len() > self.max_entries
            || entries.iter().map(|entry| entry.bytes).sum::<usize>() > self.max_bytes
        {
            entries.pop_front();
        }
    }

    pub fn drop_tab(&self, tab_id: &str) {
        self.entries.lock().retain(|entry| entry.tab_id != tab_id);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.lock().len()
    }

    #[cfg(test)]
    fn charged_bytes(&self) -> usize {
        self.entries.lock().iter().map(|entry| entry.bytes).sum()
    }
}

#[cfg(test)]
mod tests;
