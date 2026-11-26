use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::ports::{OutboxClock, SystemOutboxClock};
use super::storage::{load_json_dir, remove_file_if_present, write_private_file};
use super::types::{
    opt_base64_bytes, OutboxId, OutboxLimitKind, DEDUPE_CAPACITY_FULL, DEDUPE_CONFLICT,
    PAYLOAD_TOO_LARGE,
};

const DEDUPE_KEY_DOMAIN: &[u8] = b"domios-cloud-dedupe-key-v1";
pub const DEFAULT_DEDUPE_MAX_ENTRIES: usize = 1024;
pub const DEFAULT_DEDUPE_MAX_TOTAL_BYTES: usize = 8 * 1024 * 1024;
pub const DEFAULT_DEDUPE_MAX_REPLY_BYTES: usize = 1024 * 1024;
pub const DEFAULT_DEDUPE_TTL_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DedupeKey {
    pub account_id: String,
    pub source_installation_id: String,
    pub target_installation_id: String,
    pub id: OutboxId,
}

impl DedupeKey {
    pub fn validate(&self) -> Result<(), DedupeError> {
        for (field, value) in [
            ("account_id", self.account_id.as_str()),
            (
                "source_installation_id",
                self.source_installation_id.as_str(),
            ),
            (
                "target_installation_id",
                self.target_installation_id.as_str(),
            ),
        ] {
            if value.is_empty() || value.trim() != value || value.chars().any(char::is_control) {
                return Err(DedupeError::InvalidRequest {
                    reason: format!("{field} must be non-empty without control characters"),
                });
            }
        }
        Ok(())
    }

    pub fn accounted_bytes(&self) -> usize {
        self.account_id
            .len()
            .saturating_add(self.source_installation_id.len())
            .saturating_add(self.target_installation_id.len())
            .saturating_add(self.id.as_str().len())
    }

    pub fn storage_file_name(&self) -> String {
        let mut hasher = Sha256::new();
        hash_field(&mut hasher, DEDUPE_KEY_DOMAIN);
        hash_field(&mut hasher, self.account_id.as_bytes());
        hash_field(&mut hasher, self.source_installation_id.as_bytes());
        hash_field(&mut hasher, self.target_installation_id.as_bytes());
        hash_field(&mut hasher, self.id.as_str().as_bytes());
        format!("{:x}.json", hasher.finalize())
    }
}

fn hash_field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DedupeLimits {
    pub max_entries: usize,
    pub max_total_bytes: usize,
    pub max_reply_bytes: usize,
    pub ttl_ms: u64,
}

impl Default for DedupeLimits {
    fn default() -> Self {
        Self {
            max_entries: DEFAULT_DEDUPE_MAX_ENTRIES,
            max_total_bytes: DEFAULT_DEDUPE_MAX_TOTAL_BYTES,
            max_reply_bytes: DEFAULT_DEDUPE_MAX_REPLY_BYTES,
            ttl_ms: DEFAULT_DEDUPE_TTL_MS,
        }
    }
}

impl DedupeLimits {
    pub fn validate(&self) -> Result<(), DedupeError> {
        if self.max_entries == 0
            || self.max_total_bytes == 0
            || self.max_reply_bytes == 0
            || self.ttl_ms == 0
        {
            return Err(DedupeError::InvalidRequest {
                reason: "dedupe limits and ttl_ms must be greater than zero".into(),
            });
        }
        if self.max_reply_bytes > self.max_total_bytes {
            return Err(DedupeError::InvalidRequest {
                reason: "max_reply_bytes cannot exceed max_total_bytes".into(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DedupeEntryState {
    Reserved {
        reserved_at_ms: u64,
    },
    Completed {
        reserved_at_ms: u64,
        completed_at_ms: u64,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            with = "opt_base64_bytes"
        )]
        reply_body: Option<Vec<u8>>,
    },
    CrashUncertain {
        reserved_at_ms: u64,
        recovered_at_ms: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DedupeRecord {
    pub version: u8,
    pub key: DedupeKey,
    pub payload_digest: String,
    pub expires_at_ms: u64,
    pub state: DedupeEntryState,
}

impl DedupeRecord {
    pub const CURRENT_VERSION: u8 = 1;

    pub fn accounted_bytes(&self) -> usize {
        let reply_bytes = match &self.state {
            DedupeEntryState::Completed { reply_body, .. } => {
                reply_body.as_ref().map_or(0, Vec::len)
            }
            DedupeEntryState::Reserved { .. } | DedupeEntryState::CrashUncertain { .. } => 0,
        };
        self.key
            .accounted_bytes()
            .saturating_add(self.payload_digest.len())
            .saturating_add(reply_bytes)
    }

    pub fn is_expired(&self, now_ms: u64) -> bool {
        now_ms >= self.expires_at_ms
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DedupeDecision {
    Execute,
    DuplicateCompleted { reply_body: Option<Vec<u8>> },
    InFlightUncertain,
    CrashUncertain,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum DedupeError {
    DigestConflict {
        expected: String,
        actual: String,
    },
    CapacityFull {
        limit_kind: OutboxLimitKind,
        current: usize,
        limit: usize,
    },
    ReplyTooLarge {
        bytes: usize,
        max_bytes: usize,
    },
    InvalidState {
        reason: String,
    },
    InvalidRequest {
        reason: String,
    },
    Storage {
        detail: String,
    },
}

impl DedupeError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::DigestConflict { .. } => DEDUPE_CONFLICT,
            Self::CapacityFull { .. } => DEDUPE_CAPACITY_FULL,
            Self::ReplyTooLarge { .. } => PAYLOAD_TOO_LARGE,
            Self::InvalidState { .. } => "dedupe_invalid_state",
            Self::InvalidRequest { .. } => "invalid_request",
            Self::Storage { .. } => "dedupe_storage_error",
        }
    }
}

impl fmt::Display for DedupeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DigestConflict { expected, actual } => write!(
                f,
                "{}: digest mismatch (expected {expected}, got {actual})",
                self.code()
            ),
            Self::CapacityFull {
                limit_kind,
                current,
                limit,
            } => write!(
                f,
                "{}: {:?} limit reached ({current}/{limit})",
                self.code(),
                limit_kind
            ),
            Self::ReplyTooLarge { bytes, max_bytes } => {
                write!(f, "{}: reply {bytes} exceeds {max_bytes}", self.code())
            }
            Self::InvalidState { reason } | Self::InvalidRequest { reason } => {
                write!(f, "{}: {reason}", self.code())
            }
            Self::Storage { detail } => write!(f, "{}: {detail}", self.code()),
        }
    }
}

impl std::error::Error for DedupeError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReservePlan {
    PersistAndExecute(Box<DedupeRecord>),
    Immediate(DedupeDecision),
}

#[derive(Debug)]
struct PendingReserve {
    payload_digest: String,
    bytes: usize,
}

#[derive(Debug)]
pub struct DedupeModel {
    limits: DedupeLimits,
    entries: HashMap<DedupeKey, DedupeRecord>,
    total_bytes: usize,
    pending_reserves: HashMap<DedupeKey, PendingReserve>,
    pending_bytes: usize,
}

impl DedupeModel {
    pub fn new(limits: DedupeLimits) -> Result<Self, DedupeError> {
        limits.validate()?;
        Ok(Self {
            limits,
            entries: HashMap::new(),
            total_bytes: 0,
            pending_reserves: HashMap::new(),
            pending_bytes: 0,
        })
    }

    pub fn from_recovered(
        limits: DedupeLimits,
        records: Vec<DedupeRecord>,
        now_ms: u64,
    ) -> Result<(Self, Vec<DedupeKey>, Vec<DedupeRecord>), DedupeError> {
        let mut model = Self::new(limits)?;
        let mut expired_keys = Vec::new();
        let mut recovered_uncertain = Vec::new();

        for mut record in records {
            if record.is_expired(now_ms) {
                expired_keys.push(record.key);
                continue;
            }
            if let DedupeEntryState::Reserved { reserved_at_ms } = record.state {
                record.state = DedupeEntryState::CrashUncertain {
                    reserved_at_ms,
                    recovered_at_ms: now_ms,
                };
                recovered_uncertain.push(record.clone());
            }
            model.total_bytes = model.total_bytes.saturating_add(record.accounted_bytes());
            model.entries.insert(record.key.clone(), record);
        }

        Ok((model, expired_keys, recovered_uncertain))
    }

    pub fn prune_expired(&mut self, now_ms: u64) -> Vec<DedupeKey> {
        let expired: Vec<DedupeKey> = self
            .entries
            .iter()
            .filter(|(_, record)| record.is_expired(now_ms))
            .map(|(key, _)| key.clone())
            .collect();
        for key in &expired {
            if let Some(removed) = self.entries.remove(key) {
                self.total_bytes = self.total_bytes.saturating_sub(removed.accounted_bytes());
            }
        }
        expired
    }

    pub fn plan_reserve(
        &mut self,
        key: DedupeKey,
        payload_digest: String,
        now_ms: u64,
    ) -> Result<(ReservePlan, Vec<DedupeKey>), DedupeError> {
        key.validate()?;
        if payload_digest.is_empty() {
            return Err(DedupeError::InvalidRequest {
                reason: "payload_digest must be non-empty".into(),
            });
        }
        let expired = self.prune_expired(now_ms);

        if let Some(pending) = self.pending_reserves.get(&key) {
            if pending.payload_digest != payload_digest {
                return Err(DedupeError::DigestConflict {
                    expected: pending.payload_digest.clone(),
                    actual: payload_digest,
                });
            }
            return Ok((
                ReservePlan::Immediate(DedupeDecision::InFlightUncertain),
                expired,
            ));
        }

        if let Some(existing) = self.entries.get(&key) {
            if existing.payload_digest != payload_digest {
                return Err(DedupeError::DigestConflict {
                    expected: existing.payload_digest.clone(),
                    actual: payload_digest,
                });
            }
            let decision = match &existing.state {
                DedupeEntryState::Completed { reply_body, .. } => {
                    DedupeDecision::DuplicateCompleted {
                        reply_body: reply_body.clone(),
                    }
                }
                DedupeEntryState::Reserved { .. } => DedupeDecision::InFlightUncertain,
                DedupeEntryState::CrashUncertain { .. } => DedupeDecision::CrashUncertain,
            };
            return Ok((ReservePlan::Immediate(decision), expired));
        }

        let occupied_entries = self
            .entries
            .len()
            .saturating_add(self.pending_reserves.len());
        if occupied_entries >= self.limits.max_entries {
            return Err(DedupeError::CapacityFull {
                limit_kind: OutboxLimitKind::Count,
                current: occupied_entries,
                limit: self.limits.max_entries,
            });
        }

        let record = DedupeRecord {
            version: DedupeRecord::CURRENT_VERSION,
            key: key.clone(),
            payload_digest: payload_digest.clone(),
            expires_at_ms: now_ms.saturating_add(self.limits.ttl_ms),
            state: DedupeEntryState::Reserved {
                reserved_at_ms: now_ms,
            },
        };
        let bytes = record.accounted_bytes();
        let occupied_bytes = self.total_bytes.saturating_add(self.pending_bytes);
        if occupied_bytes.saturating_add(bytes) > self.limits.max_total_bytes {
            return Err(DedupeError::CapacityFull {
                limit_kind: OutboxLimitKind::Bytes,
                current: occupied_bytes,
                limit: self.limits.max_total_bytes,
            });
        }

        self.pending_bytes = self.pending_bytes.saturating_add(bytes);
        self.pending_reserves.insert(
            key,
            PendingReserve {
                payload_digest,
                bytes,
            },
        );
        Ok((ReservePlan::PersistAndExecute(Box::new(record)), expired))
    }

    pub fn commit_reserve(&mut self, record: DedupeRecord) {
        if let Some(pending) = self.pending_reserves.remove(&record.key) {
            self.pending_bytes = self.pending_bytes.saturating_sub(pending.bytes);
        }
        self.total_bytes = self.total_bytes.saturating_add(record.accounted_bytes());
        self.entries.insert(record.key.clone(), record);
    }

    pub fn abort_reserve(&mut self, key: &DedupeKey) {
        if let Some(pending) = self.pending_reserves.remove(key) {
            self.pending_bytes = self.pending_bytes.saturating_sub(pending.bytes);
        }
    }

    pub fn prepare_complete(
        &self,
        key: &DedupeKey,
        reply_body: Option<Vec<u8>>,
        now_ms: u64,
    ) -> Result<DedupeRecord, DedupeError> {
        let reply_bytes = reply_body.as_ref().map_or(0, Vec::len);
        if reply_bytes > self.limits.max_reply_bytes {
            return Err(DedupeError::ReplyTooLarge {
                bytes: reply_bytes,
                max_bytes: self.limits.max_reply_bytes,
            });
        }
        let existing = self
            .entries
            .get(key)
            .ok_or_else(|| DedupeError::InvalidState {
                reason: "cannot complete unreserved dedupe key".into(),
            })?;
        let reserved_at_ms = match existing.state {
            DedupeEntryState::Reserved { reserved_at_ms } => reserved_at_ms,
            DedupeEntryState::Completed { .. } => {
                return Err(DedupeError::InvalidState {
                    reason: "dedupe key is already completed".into(),
                });
            }
            DedupeEntryState::CrashUncertain { .. } => {
                return Err(DedupeError::InvalidState {
                    reason: "cannot complete crash_uncertain dedupe key".into(),
                });
            }
        };
        let occupied_bytes = self.total_bytes.saturating_add(self.pending_bytes);
        if occupied_bytes.saturating_add(reply_bytes) > self.limits.max_total_bytes {
            return Err(DedupeError::CapacityFull {
                limit_kind: OutboxLimitKind::Bytes,
                current: occupied_bytes,
                limit: self.limits.max_total_bytes,
            });
        }
        Ok(DedupeRecord {
            version: DedupeRecord::CURRENT_VERSION,
            key: existing.key.clone(),
            payload_digest: existing.payload_digest.clone(),
            expires_at_ms: existing.expires_at_ms,
            state: DedupeEntryState::Completed {
                reserved_at_ms,
                completed_at_ms: now_ms,
                reply_body,
            },
        })
    }

    pub fn commit_complete(&mut self, updated: DedupeRecord) -> Result<(), DedupeError> {
        let Some(existing) = self.entries.get(&updated.key) else {
            return Err(DedupeError::InvalidState {
                reason: "cannot commit completion for missing dedupe key".into(),
            });
        };
        if !matches!(existing.state, DedupeEntryState::Reserved { .. }) {
            return Err(DedupeError::InvalidState {
                reason: "cannot commit completion for non-reserved dedupe key".into(),
            });
        }
        let previous_bytes = existing.accounted_bytes();
        self.total_bytes = self
            .total_bytes
            .saturating_sub(previous_bytes)
            .saturating_add(updated.accounted_bytes());
        self.entries.insert(updated.key.clone(), updated);
        Ok(())
    }

    pub fn get(&self, key: &DedupeKey) -> Option<&DedupeRecord> {
        self.entries.get(key)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }
}

pub struct ReceiverDedupe {
    dir: PathBuf,
    clock: Arc<dyn OutboxClock>,
    model: Mutex<DedupeModel>,
}

impl ReceiverDedupe {
    pub async fn open(
        dir: impl Into<PathBuf>,
        limits: DedupeLimits,
        clock: Arc<dyn OutboxClock>,
    ) -> Result<Self, DedupeError> {
        limits.validate()?;
        let dir = dir.into();
        let now_ms = clock.now_ms();
        let load_dir = dir.clone();
        let records: Vec<DedupeRecord> = tokio::task::spawn_blocking(move || {
            codeterm_util::private_file::create_dir_all(&load_dir).map_err(|error| {
                DedupeError::Storage {
                    detail: error.to_string(),
                }
            })?;
            load_json_dir::<DedupeRecord>(&load_dir).map_err(|error| DedupeError::Storage {
                detail: error.to_string(),
            })
        })
        .await
        .map_err(|error| DedupeError::Storage {
            detail: format!("dedupe load task failed: {error}"),
        })??;

        let (model, expired_keys, recovered_uncertain) =
            DedupeModel::from_recovered(limits, records, now_ms)?;

        if !expired_keys.is_empty() || !recovered_uncertain.is_empty() {
            let sync_dir = dir.clone();
            tokio::task::spawn_blocking(move || -> Result<(), DedupeError> {
                for key in expired_keys {
                    let path = sync_dir.join(key.storage_file_name());
                    let _ = remove_file_if_present(&path);
                }
                for record in recovered_uncertain {
                    persist_dedupe_record_sync(&sync_dir, &record)?;
                }
                Ok(())
            })
            .await
            .map_err(|error| DedupeError::Storage {
                detail: format!("dedupe recovery task failed: {error}"),
            })??;
        }

        Ok(Self {
            dir,
            clock,
            model: Mutex::new(model),
        })
    }

    pub async fn open_with_system_clock(
        dir: impl Into<PathBuf>,
        limits: DedupeLimits,
    ) -> Result<Self, DedupeError> {
        Self::open(dir, limits, Arc::new(SystemOutboxClock)).await
    }

    pub async fn reserve(
        &self,
        key: DedupeKey,
        payload_digest: String,
    ) -> Result<DedupeDecision, DedupeError> {
        let now_ms = self.clock.now_ms();
        let (plan, expired) = {
            let mut model = self.model.lock();
            model.plan_reserve(key, payload_digest, now_ms)?
        };

        if !expired.is_empty() {
            let dir = self.dir.clone();
            let _ = tokio::task::spawn_blocking(move || {
                for expired_key in expired {
                    let _ = remove_file_if_present(&dir.join(expired_key.storage_file_name()));
                }
            })
            .await;
        }

        match plan {
            ReservePlan::Immediate(decision) => Ok(decision),
            ReservePlan::PersistAndExecute(record) => {
                let record = *record;
                let dir = self.dir.clone();
                let to_write = record.clone();
                let write_result = tokio::task::spawn_blocking(move || {
                    persist_dedupe_record_sync(&dir, &to_write)
                })
                .await
                .map_err(|error| DedupeError::Storage {
                    detail: format!("dedupe reserve task failed: {error}"),
                })?;

                let mut model = self.model.lock();
                match write_result {
                    Ok(()) => {
                        model.commit_reserve(record);
                        Ok(DedupeDecision::Execute)
                    }
                    Err(error) => {
                        model.abort_reserve(&record.key);
                        Err(error)
                    }
                }
            }
        }
    }

    pub async fn complete(
        &self,
        key: &DedupeKey,
        reply_body: Option<Vec<u8>>,
    ) -> Result<(), DedupeError> {
        let now_ms = self.clock.now_ms();
        let updated = {
            let model = self.model.lock();
            model.prepare_complete(key, reply_body, now_ms)?
        };

        let dir = self.dir.clone();
        let to_write = updated.clone();
        tokio::task::spawn_blocking(move || persist_dedupe_record_sync(&dir, &to_write))
            .await
            .map_err(|error| DedupeError::Storage {
                detail: format!("dedupe complete task failed: {error}"),
            })??;

        let mut model = self.model.lock();
        model.commit_complete(updated)
    }

    pub fn record(&self, key: &DedupeKey) -> Option<DedupeRecord> {
        self.model.lock().get(key).cloned()
    }

    pub fn len(&self) -> usize {
        self.model.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.model.lock().is_empty()
    }
}

fn persist_dedupe_record_sync(dir: &Path, record: &DedupeRecord) -> Result<(), DedupeError> {
    let bytes = serde_json::to_vec(record).map_err(|error| DedupeError::Storage {
        detail: format!("dedupe serialization failed: {error}"),
    })?;
    let path = dir.join(record.key.storage_file_name());
    write_private_file(&path, &bytes).map_err(|error| DedupeError::Storage {
        detail: error.to_string(),
    })
}
