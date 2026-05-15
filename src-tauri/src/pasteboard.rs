//! In-memory cross-mesh pastebin. Ephemeral handoff blobs keyed by a short
//! slug ("paste:a3f9k2"). Hard-capped at 20 entries with oldest-first
//! eviction; TTL default 1h, max 24h. No disk persistence — daemon restart
//! drops every paste, which is the correct semantics for "drop a payload,
//! get a slug, let any peer fetch it once, then forget." See issue #98.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::RwLock;
use rand::Rng;
use serde::{Deserialize, Serialize};

pub const MAX_ENTRIES: usize = 20;
pub const DEFAULT_TTL_MS: u64 = 60 * 60 * 1000; // 1h
pub const MAX_TTL_MS: u64 = 24 * 60 * 60 * 1000; // 24h
pub const ID_PREFIX: &str = "paste:";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasteEntry {
    pub id: String,
    /// Body bytes. Serialized as a UTF-8 string when the payload is valid
    /// UTF-8; otherwise the API returns an explicit binary marker so
    /// the CLI can fall back to base64 (not implemented yet — body is
    /// expected to be text for the canonical use case).
    pub body: Vec<u8>,
    pub content_type: String,
    pub created_at: u64,
    pub expires_at: u64,
    pub pop_on_read: bool,
    pub source_peer: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PasteListEntry {
    pub id: String,
    pub size: usize,
    pub content_type: String,
    pub created_at: u64,
    pub expires_at: u64,
    pub ttl_remaining_ms: u64,
    pub pop_on_read: bool,
    pub source_peer: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PutParams {
    pub body: String,
    #[serde(default)]
    pub content_type: Option<String>,
    /// TTL in milliseconds. None → DEFAULT_TTL_MS. Capped at MAX_TTL_MS.
    #[serde(default)]
    pub ttl_ms: Option<u64>,
    #[serde(default)]
    pub pop_on_read: Option<bool>,
    #[serde(default)]
    pub source_peer: Option<String>,
}

#[derive(Debug)]
pub enum PasteError {
    NotFound(String),
    Empty,
}

impl std::fmt::Display for PasteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "paste not found: {}", id),
            Self::Empty => write!(f, "paste body is empty"),
        }
    }
}

impl std::error::Error for PasteError {}

#[derive(Clone, Default)]
pub struct PasteboardState {
    inner: Arc<RwLock<HashMap<String, PasteEntry>>>,
}

impl PasteboardState {
    pub fn new() -> Self {
        Self { inner: Arc::new(RwLock::new(HashMap::new())) }
    }

    pub fn put(&self, params: PutParams) -> Result<PasteEntry, PasteError> {
        if params.body.is_empty() {
            return Err(PasteError::Empty);
        }
        let now = now_ms();
        let ttl = params.ttl_ms.unwrap_or(DEFAULT_TTL_MS).min(MAX_TTL_MS);
        let id = format!("{}{}", ID_PREFIX, short_slug());
        let entry = PasteEntry {
            id: id.clone(),
            body: params.body.into_bytes(),
            content_type: params.content_type.unwrap_or_else(|| "text/plain".into()),
            created_at: now,
            expires_at: now + ttl,
            pop_on_read: params.pop_on_read.unwrap_or(false),
            source_peer: params.source_peer,
        };
        let mut map = self.inner.write();
        self.gc_locked(&mut map, now);
        if map.len() >= MAX_ENTRIES {
            // Evict oldest by created_at.
            if let Some(oldest_id) = map
                .values()
                .min_by_key(|e| e.created_at)
                .map(|e| e.id.clone())
            {
                map.remove(&oldest_id);
            }
        }
        map.insert(id, entry.clone());
        Ok(entry)
    }

    /// Fetch (and optionally pop) a paste. Returns None if the id is unknown
    /// or expired.
    pub fn get(&self, id: &str, pop: bool) -> Option<PasteEntry> {
        let now = now_ms();
        let mut map = self.inner.write();
        self.gc_locked(&mut map, now);
        let entry = map.get(id)?.clone();
        if entry.expires_at <= now {
            map.remove(id);
            return None;
        }
        if pop || entry.pop_on_read {
            map.remove(id);
        }
        Some(entry)
    }

    pub fn remove(&self, id: &str) -> Result<(), PasteError> {
        let mut map = self.inner.write();
        if map.remove(id).is_some() {
            Ok(())
        } else {
            Err(PasteError::NotFound(id.to_string()))
        }
    }

    pub fn list(&self) -> Vec<PasteListEntry> {
        let now = now_ms();
        let mut map = self.inner.write();
        self.gc_locked(&mut map, now);
        let mut out: Vec<PasteListEntry> = map
            .values()
            .map(|e| PasteListEntry {
                id: e.id.clone(),
                size: e.body.len(),
                content_type: e.content_type.clone(),
                created_at: e.created_at,
                expires_at: e.expires_at,
                ttl_remaining_ms: e.expires_at.saturating_sub(now),
                pop_on_read: e.pop_on_read,
                source_peer: e.source_peer.clone(),
            })
            .collect();
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        out
    }

    /// Sweep expired entries. Called from a background tick task and
    /// opportunistically on every mutating call so a long-idle pasteboard
    /// still self-cleans on the next read.
    pub fn gc(&self) {
        let now = now_ms();
        let mut map = self.inner.write();
        self.gc_locked(&mut map, now);
    }

    fn gc_locked(&self, map: &mut HashMap<String, PasteEntry>, now: u64) {
        map.retain(|_, e| e.expires_at > now);
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn short_slug() -> String {
    const ALPHABET: &[u8] = b"abcdefghijkmnpqrstuvwxyz23456789";
    let mut rng = rand::thread_rng();
    (0..6)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_text(pb: &PasteboardState, body: &str) -> PasteEntry {
        pb.put(PutParams {
            body: body.into(),
            content_type: None,
            ttl_ms: None,
            pop_on_read: None,
            source_peer: None,
        })
        .unwrap()
    }

    #[test]
    fn put_then_get_roundtrips_body() {
        let pb = PasteboardState::new();
        let entry = put_text(&pb, "hello world");
        let fetched = pb.get(&entry.id, false).expect("entry must be retrievable");
        assert_eq!(fetched.body, b"hello world");
        // Without pop, second get still works.
        assert!(pb.get(&entry.id, false).is_some());
    }

    #[test]
    fn pop_on_read_consumes_entry() {
        let pb = PasteboardState::new();
        let entry = pb.put(PutParams {
            body: "one-shot".into(),
            pop_on_read: Some(true),
            ..Default::default()
        }).unwrap();
        assert!(pb.get(&entry.id, false).is_some(), "first read returns the entry");
        assert!(pb.get(&entry.id, false).is_none(), "second read finds nothing");
    }

    #[test]
    fn explicit_pop_deletes_even_when_flag_unset() {
        let pb = PasteboardState::new();
        let entry = put_text(&pb, "blob");
        assert!(pb.get(&entry.id, true).is_some());
        assert!(pb.get(&entry.id, false).is_none());
    }

    #[test]
    fn cap_evicts_oldest_first() {
        let pb = PasteboardState::new();
        let mut ids = Vec::new();
        for i in 0..MAX_ENTRIES {
            let e = put_text(&pb, &format!("payload-{i}"));
            ids.push(e.id);
            // Spread the created_at so oldest is unambiguous.
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let oldest = ids.first().unwrap().clone();
        // One more push triggers eviction of the oldest.
        std::thread::sleep(std::time::Duration::from_millis(2));
        put_text(&pb, "overflow");
        assert_eq!(pb.list().len(), MAX_ENTRIES);
        assert!(pb.get(&oldest, false).is_none(), "oldest must be evicted");
    }

    #[test]
    fn ttl_is_clamped_to_max() {
        let pb = PasteboardState::new();
        let huge = MAX_TTL_MS + 5 * 60 * 60 * 1000;
        let entry = pb.put(PutParams {
            body: "X".into(),
            ttl_ms: Some(huge),
            ..Default::default()
        }).unwrap();
        let span = entry.expires_at.saturating_sub(entry.created_at);
        assert!(span <= MAX_TTL_MS + 1, "TTL must be clamped to 24h, got {}", span);
    }

    #[test]
    fn empty_body_rejected() {
        let pb = PasteboardState::new();
        assert!(matches!(
            pb.put(PutParams { body: String::new(), ..Default::default() }),
            Err(PasteError::Empty),
        ));
    }

    #[test]
    fn gc_removes_expired_entries() {
        let pb = PasteboardState::new();
        let entry = pb.put(PutParams {
            body: "blip".into(),
            ttl_ms: Some(10),
            ..Default::default()
        }).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        pb.gc();
        assert!(pb.get(&entry.id, false).is_none());
        assert!(pb.list().is_empty());
    }

    #[test]
    fn ids_are_unique_under_burst() {
        let pb = PasteboardState::new();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..10 {
            let e = put_text(&pb, "x");
            assert!(seen.insert(e.id), "slug collision in burst");
        }
    }
}
