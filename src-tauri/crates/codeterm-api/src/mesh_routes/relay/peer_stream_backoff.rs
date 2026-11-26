use codeterm_service_core::MeshStateHandle;
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) const BASE_DELAY_MS: u64 = 1_000;
pub(crate) const MAX_DELAY_MS: u64 = 30_000;
pub(crate) const REFRESH_COOLDOWN_MS: u64 = 10_000;
const MAX_ENTRIES: usize = 256;

pub(crate) type BackoffKey = (usize, String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RefusalOutcome {
    pub delay_ms: u64,
    pub retry_at_ms: u64,
    pub attempts: u32,
    pub trigger_refresh: bool,
}

struct Entry {
    attempts: u32,
    retry_at_ms: u64,
    link_nonce: Option<String>,
    last_refresh_ms: Option<u64>,
    updated_at_ms: u64,
}

pub(crate) struct PeerStreamBackoff {
    base_delay_ms: u64,
    max_delay_ms: u64,
    refresh_cooldown_ms: u64,
    max_entries: usize,
    entries: HashMap<BackoffKey, Entry>,
}

impl PeerStreamBackoff {
    pub(crate) fn new(
        base_delay_ms: u64,
        max_delay_ms: u64,
        refresh_cooldown_ms: u64,
        max_entries: usize,
    ) -> Self {
        let base = base_delay_ms.max(1);
        Self {
            base_delay_ms: base,
            max_delay_ms: max_delay_ms.max(base),
            refresh_cooldown_ms: refresh_cooldown_ms.max(1),
            max_entries: max_entries.max(1),
            entries: HashMap::new(),
        }
    }

    pub(crate) fn delay_ms(&self, attempts: u32, jitter_unit: f64) -> u64 {
        let shift = attempts.saturating_sub(1).min(16);
        let ceiling = self
            .base_delay_ms
            .saturating_mul(1u64 << shift)
            .min(self.max_delay_ms);
        let unit = if jitter_unit.is_finite() {
            jitter_unit.clamp(0.0, 1.0)
        } else {
            0.5
        };
        let factor = 0.5 + 0.5 * unit;
        ((ceiling as f64) * factor).round().max(1.0) as u64
    }

    pub(crate) fn check_allowed(
        &mut self,
        key: &BackoffKey,
        link_nonce: Option<&str>,
        now_ms: u64,
    ) -> Result<(), u64> {
        let Some(entry) = self.entries.get(key) else {
            return Ok(());
        };
        if entry.link_nonce.as_deref() != link_nonce {
            self.entries.remove(key);
            return Ok(());
        }
        if now_ms < entry.retry_at_ms {
            return Err(entry.retry_at_ms - now_ms);
        }
        Ok(())
    }

    pub(crate) fn record_refusal(
        &mut self,
        key: BackoffKey,
        link_nonce: Option<&str>,
        now_ms: u64,
        jitter_unit: f64,
    ) -> RefusalOutcome {
        if !self.entries.contains_key(&key) && self.entries.len() >= self.max_entries {
            self.entries.retain(|_, entry| now_ms < entry.retry_at_ms);
            if self.entries.len() >= self.max_entries {
                if let Some(oldest) = self
                    .entries
                    .iter()
                    .min_by_key(|(_, entry)| entry.updated_at_ms)
                    .map(|(k, _)| k.clone())
                {
                    self.entries.remove(&oldest);
                }
            }
        }
        let next_attempts = self
            .entries
            .get(&key)
            .filter(|entry| entry.link_nonce.as_deref() == link_nonce)
            .map(|entry| entry.attempts)
            .unwrap_or(0)
            .saturating_add(1);
        let delay_ms = self.delay_ms(next_attempts, jitter_unit);
        let retry_at_ms = now_ms.saturating_add(delay_ms);
        let entry = self.entries.entry(key).or_insert_with(|| Entry {
            attempts: 0,
            retry_at_ms: 0,
            link_nonce: link_nonce.map(str::to_owned),
            last_refresh_ms: None,
            updated_at_ms: now_ms,
        });
        entry.attempts = next_attempts;
        entry.retry_at_ms = retry_at_ms;
        entry.link_nonce = link_nonce.map(str::to_owned);
        entry.updated_at_ms = now_ms;
        let trigger_refresh = entry
            .last_refresh_ms
            .is_none_or(|last| now_ms.saturating_sub(last) >= self.refresh_cooldown_ms);
        if trigger_refresh {
            entry.last_refresh_ms = Some(now_ms);
        }
        RefusalOutcome {
            delay_ms,
            retry_at_ms,
            attempts: next_attempts,
            trigger_refresh,
        }
    }

    pub(crate) fn record_success(&mut self, key: &BackoffKey) {
        self.entries.remove(key);
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

static STREAM_BACKOFF: LazyLock<Mutex<PeerStreamBackoff>> = LazyLock::new(|| {
    Mutex::new(PeerStreamBackoff::new(
        BASE_DELAY_MS,
        MAX_DELAY_MS,
        REFRESH_COOLDOWN_MS,
        MAX_ENTRIES,
    ))
});

fn key_for(mesh: &MeshStateHandle, peer_id: &str) -> BackoffKey {
    (Arc::as_ptr(mesh) as usize, peer_id.to_owned())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn jitter_unit() -> f64 {
    let bytes = *uuid::Uuid::new_v4().as_bytes();
    let sample = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    f64::from(sample) / f64::from(u32::MAX)
}

pub(super) fn check_allowed(
    mesh: &MeshStateHandle,
    peer_id: &str,
    link_nonce: Option<&str>,
) -> Result<(), u64> {
    let key = key_for(mesh, peer_id);
    STREAM_BACKOFF
        .lock()
        .check_allowed(&key, link_nonce, now_ms())
}

pub(super) fn record_refusal(
    mesh: &MeshStateHandle,
    peer_id: &str,
    link_nonce: Option<&str>,
) -> RefusalOutcome {
    let key = key_for(mesh, peer_id);
    STREAM_BACKOFF
        .lock()
        .record_refusal(key, link_nonce, now_ms(), jitter_unit())
}

pub(super) fn record_success(mesh: &MeshStateHandle, peer_id: &str) {
    let key = key_for(mesh, peer_id);
    STREAM_BACKOFF.lock().record_success(&key);
}

#[cfg(test)]
mod tests;
