use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use super::types::{
    CompletedApplicationReply, InstallationPair, OutboundPeerRequest, OutboxEnqueueReceipt,
    OutboxError, OutboxFrame, OutboxId, OutboxLimitKind, OutboxLimits, OutboxRecordStatus,
    OutboxSnapshot, StreamDeliveryOutcome, UnconfirmedReason,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum PersistedActiveState {
    Queued,
    InFlight { stream_id: u64, claimed_at_ms: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedOutboxRecord {
    pub version: u8,
    pub frame: OutboxFrame,
    pub state: PersistedActiveState,
    pub accounted_bytes: usize,
}

impl PersistedOutboxRecord {
    pub const CURRENT_VERSION: u8 = 1;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedUnconfirmedReceipt {
    pub version: u8,
    pub id: OutboxId,
    pub sequence: u64,
    pub account_id: String,
    pub source_installation_id: String,
    pub target_installation_id: String,
    pub payload_digest: String,
    pub reason: UnconfirmedReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes_written: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub settled_at_ms: u64,
}

impl PersistedUnconfirmedReceipt {
    pub const CURRENT_VERSION: u8 = 1;

    pub fn into_status(&self) -> OutboxRecordStatus {
        OutboxRecordStatus::DeliveryUnconfirmed {
            sequence: self.sequence,
            reason: self.reason,
            bytes_written: StreamDeliveryOutcome::normalize_unconfirmed_bytes_written(
                self.bytes_written,
            ),
            detail: self.detail.clone(),
            settled_at_ms: self.settled_at_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedSettledReceipt {
    pub version: u8,
    pub id: OutboxId,
    pub sequence: u64,
    pub status: OutboxRecordStatus,
}

impl PersistedSettledReceipt {
    pub const CURRENT_VERSION: u8 = 1;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveRuntimeState {
    Queued,
    Evaluating { stream_id: u64 },
    InFlight { stream_id: u64, claimed_at_ms: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveEntry {
    pub frame: OutboxFrame,
    pub state: ActiveRuntimeState,
    pub accounted_bytes: usize,
}

impl ActiveEntry {
    pub fn to_persisted(&self) -> PersistedOutboxRecord {
        let state = match self.state {
            ActiveRuntimeState::Queued | ActiveRuntimeState::Evaluating { .. } => {
                PersistedActiveState::Queued
            }
            ActiveRuntimeState::InFlight {
                stream_id,
                claimed_at_ms,
            } => PersistedActiveState::InFlight {
                stream_id,
                claimed_at_ms,
            },
        };
        PersistedOutboxRecord {
            version: PersistedOutboxRecord::CURRENT_VERSION,
            frame: self.frame.clone(),
            state,
            accounted_bytes: self.accounted_bytes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachedStream {
    pub pair: InstallationPair,
    pub account_id: String,
    pub stream_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnqueueReservation {
    pub records: Vec<PersistedOutboxRecord>,
    pub total_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NextDrainAction {
    Evaluate(Box<OutboxFrame>),
    WaitForInFlight,
    Idle,
    StreamSuperseded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredCrashTransition {
    pub stale_active_sequence: u64,
    pub unconfirmed: PersistedUnconfirmedReceipt,
}

#[derive(Debug)]
pub struct OutboxModel {
    limits: OutboxLimits,
    next_sequence: u64,
    active: BTreeMap<u64, ActiveEntry>,
    active_by_id: HashMap<OutboxId, u64>,
    active_bytes: usize,
    reserved_count: usize,
    reserved_bytes: usize,
    unconfirmed: BTreeMap<OutboxId, PersistedUnconfirmedReceipt>,
    settled_order: VecDeque<OutboxId>,
    settled_by_id: HashMap<OutboxId, PersistedSettledReceipt>,
    replies: HashMap<OutboxId, CompletedApplicationReply>,
    streams: HashMap<InstallationPair, AttachedStream>,
}

impl OutboxModel {
    pub fn new(limits: OutboxLimits) -> Result<Self, OutboxError> {
        limits.validate()?;
        Ok(Self {
            limits,
            next_sequence: 1,
            active: BTreeMap::new(),
            active_by_id: HashMap::new(),
            active_bytes: 0,
            reserved_count: 0,
            reserved_bytes: 0,
            unconfirmed: BTreeMap::new(),
            settled_order: VecDeque::new(),
            settled_by_id: HashMap::new(),
            replies: HashMap::new(),
            streams: HashMap::new(),
        })
    }

    pub fn from_recovered(
        limits: OutboxLimits,
        active_records: Vec<PersistedOutboxRecord>,
        unconfirmed_receipts: Vec<PersistedUnconfirmedReceipt>,
        settled_receipts: Vec<PersistedSettledReceipt>,
        now_ms: u64,
    ) -> Result<(Self, Vec<RecoveredCrashTransition>), OutboxError> {
        let mut model = Self::new(limits)?;
        let mut max_sequence = 0u64;

        for receipt in unconfirmed_receipts {
            max_sequence = max_sequence.max(receipt.sequence);
            model.unconfirmed.insert(receipt.id.clone(), receipt);
        }

        let mut sorted_settled = settled_receipts;
        sorted_settled.sort_by_key(|receipt| receipt.sequence);
        for receipt in sorted_settled {
            max_sequence = max_sequence.max(receipt.sequence);
            model.insert_settled_bounded(receipt);
        }

        let mut sorted_active = active_records;
        sorted_active.sort_by_key(|record| record.frame.sequence);

        let mut crash_transitions = Vec::new();
        for record in sorted_active {
            max_sequence = max_sequence.max(record.frame.sequence);
            if model.unconfirmed.contains_key(&record.frame.id)
                || model.settled_by_id.contains_key(&record.frame.id)
            {
                continue;
            }
            match record.state {
                PersistedActiveState::Queued => {
                    let seq = record.frame.sequence;
                    let bytes = record.accounted_bytes;
                    model.active_bytes = model.active_bytes.saturating_add(bytes);
                    model.active_by_id.insert(record.frame.id.clone(), seq);
                    model.active.insert(
                        seq,
                        ActiveEntry {
                            frame: record.frame,
                            state: ActiveRuntimeState::Queued,
                            accounted_bytes: bytes,
                        },
                    );
                }
                PersistedActiveState::InFlight { .. } => {
                    let unconfirmed = PersistedUnconfirmedReceipt {
                        version: PersistedUnconfirmedReceipt::CURRENT_VERSION,
                        id: record.frame.id.clone(),
                        sequence: record.frame.sequence,
                        account_id: record.frame.request.account_id.clone(),
                        source_installation_id: record.frame.request.source_installation_id.clone(),
                        target_installation_id: record.frame.request.target_installation_id.clone(),
                        payload_digest: record.frame.payload_digest.clone(),
                        reason: UnconfirmedReason::ProcessCrashAfterClaim,
                        bytes_written: None,
                        detail: Some("process terminated after durable delivery claim".into()),
                        settled_at_ms: now_ms,
                    };
                    model
                        .unconfirmed
                        .insert(unconfirmed.id.clone(), unconfirmed.clone());
                    crash_transitions.push(RecoveredCrashTransition {
                        stale_active_sequence: record.frame.sequence,
                        unconfirmed,
                    });
                }
            }
        }

        model.next_sequence = max_sequence.saturating_add(1).max(1);
        Ok((model, crash_transitions))
    }

    pub fn limits(&self) -> OutboxLimits {
        self.limits
    }

    pub fn reserve_enqueue(
        &mut self,
        request: OutboundPeerRequest,
        id: Option<OutboxId>,
        now_ms: u64,
    ) -> Result<EnqueueReservation, OutboxError> {
        self.reserve_enqueue_batch(vec![(request, id)], now_ms)
    }

    pub fn reserve_broadcast(
        &mut self,
        base: OutboundPeerRequest,
        target_installation_ids: Vec<String>,
        now_ms: u64,
    ) -> Result<EnqueueReservation, OutboxError> {
        if target_installation_ids.is_empty() {
            return Err(OutboxError::InvalidRequest {
                reason: "broadcast requires at least one target_installation_id".into(),
            });
        }
        let mut seen = HashSet::with_capacity(target_installation_ids.len());
        let mut items = Vec::with_capacity(target_installation_ids.len());
        for target in target_installation_ids {
            if !seen.insert(target.clone()) {
                return Err(OutboxError::InvalidRequest {
                    reason: format!("duplicate broadcast target_installation_id: {target}"),
                });
            }
            items.push((base.retargeted(target), None));
        }
        self.reserve_enqueue_batch(items, now_ms)
    }

    pub fn reserve_enqueue_batch(
        &mut self,
        items: Vec<(OutboundPeerRequest, Option<OutboxId>)>,
        now_ms: u64,
    ) -> Result<EnqueueReservation, OutboxError> {
        if items.is_empty() {
            return Err(OutboxError::InvalidRequest {
                reason: "enqueue batch cannot be empty".into(),
            });
        }
        let mut batch_bytes = 0usize;
        let mut seen_ids = HashSet::with_capacity(items.len());
        let mut validated = Vec::with_capacity(items.len());

        for (request, maybe_id) in items {
            request.validate()?;
            let bytes = request.accounted_bytes();
            if bytes > self.limits.max_record_bytes {
                return Err(OutboxError::PayloadTooLarge {
                    bytes,
                    max_bytes: self.limits.max_record_bytes,
                });
            }
            let id = maybe_id.unwrap_or_else(OutboxId::generate);
            if !seen_ids.insert(id.clone())
                || self.active_by_id.contains_key(&id)
                || self.unconfirmed.contains_key(&id)
                || self.settled_by_id.contains_key(&id)
            {
                return Err(OutboxError::DuplicateId { id });
            }
            batch_bytes = batch_bytes.checked_add(bytes).ok_or(OutboxError::Full {
                limit_kind: OutboxLimitKind::Bytes,
                current: self.occupied_bytes(),
                limit: self.limits.max_total_bytes,
            })?;
            validated.push((request, id, bytes));
        }

        if self.unconfirmed.len() >= self.limits.max_retained_unconfirmed {
            return Err(OutboxError::Full {
                limit_kind: OutboxLimitKind::Count,
                current: self.unconfirmed.len(),
                limit: self.limits.max_retained_unconfirmed,
            });
        }

        let current_count = self.occupied_count();
        if current_count.saturating_add(validated.len()) > self.limits.max_records {
            return Err(OutboxError::Full {
                limit_kind: OutboxLimitKind::Count,
                current: current_count,
                limit: self.limits.max_records,
            });
        }

        let current_bytes = self.occupied_bytes();
        if current_bytes.saturating_add(batch_bytes) > self.limits.max_total_bytes {
            return Err(OutboxError::Full {
                limit_kind: OutboxLimitKind::Bytes,
                current: current_bytes,
                limit: self.limits.max_total_bytes,
            });
        }

        let mut records = Vec::with_capacity(validated.len());
        for (request, id, bytes) in validated {
            let sequence = self.next_sequence;
            self.next_sequence =
                self.next_sequence
                    .checked_add(1)
                    .ok_or_else(|| OutboxError::Storage {
                        detail: "outbox sequence overflow".into(),
                    })?;
            let payload_digest = request.payload_digest();
            records.push(PersistedOutboxRecord {
                version: PersistedOutboxRecord::CURRENT_VERSION,
                frame: OutboxFrame {
                    id,
                    sequence,
                    enqueued_at_ms: now_ms,
                    payload_digest,
                    request,
                },
                state: PersistedActiveState::Queued,
                accounted_bytes: bytes,
            });
        }

        self.reserved_count = self.reserved_count.saturating_add(records.len());
        self.reserved_bytes = self.reserved_bytes.saturating_add(batch_bytes);
        Ok(EnqueueReservation {
            records,
            total_bytes: batch_bytes,
        })
    }

    pub fn commit_enqueue(&mut self, reservation: EnqueueReservation) -> Vec<OutboxEnqueueReceipt> {
        self.reserved_count = self
            .reserved_count
            .saturating_sub(reservation.records.len());
        self.reserved_bytes = self.reserved_bytes.saturating_sub(reservation.total_bytes);
        let mut receipts = Vec::with_capacity(reservation.records.len());
        for record in reservation.records {
            receipts.push(OutboxEnqueueReceipt {
                id: record.frame.id.clone(),
                sequence: record.frame.sequence,
                enqueued_at_ms: record.frame.enqueued_at_ms,
                accounted_bytes: record.accounted_bytes,
                payload_digest: record.frame.payload_digest.clone(),
            });
            let seq = record.frame.sequence;
            self.active_bytes = self.active_bytes.saturating_add(record.accounted_bytes);
            self.active_by_id.insert(record.frame.id.clone(), seq);
            self.active.insert(
                seq,
                ActiveEntry {
                    frame: record.frame,
                    state: ActiveRuntimeState::Queued,
                    accounted_bytes: record.accounted_bytes,
                },
            );
        }
        receipts
    }

    pub fn abort_enqueue(&mut self, reservation: &EnqueueReservation) {
        self.reserved_count = self
            .reserved_count
            .saturating_sub(reservation.records.len());
        self.reserved_bytes = self.reserved_bytes.saturating_sub(reservation.total_bytes);
    }

    pub fn attach_stream(
        &mut self,
        pair: InstallationPair,
        account_id: String,
        stream_id: u64,
    ) -> Option<u64> {
        let previous = self.streams.insert(
            pair.clone(),
            AttachedStream {
                pair,
                account_id,
                stream_id,
            },
        );
        previous.map(|stream| stream.stream_id)
    }

    pub fn detach_stream(&mut self, stream_id: u64) -> Option<InstallationPair> {
        let pair = self
            .streams
            .iter()
            .find(|(_, stream)| stream.stream_id == stream_id)
            .map(|(pair, _)| pair.clone())?;
        self.streams.remove(&pair);
        Some(pair)
    }

    pub fn is_current_stream(&self, pair: &InstallationPair, stream_id: u64) -> bool {
        self.streams
            .get(pair)
            .is_some_and(|stream| stream.stream_id == stream_id)
    }

    pub fn begin_next_drain(
        &mut self,
        pair: &InstallationPair,
        account_id: &str,
        stream_id: u64,
    ) -> NextDrainAction {
        let Some(attached) = self.streams.get(pair) else {
            return NextDrainAction::StreamSuperseded;
        };
        if attached.stream_id != stream_id || attached.account_id != account_id {
            return NextDrainAction::StreamSuperseded;
        }
        for entry in self.active.values_mut() {
            if entry.frame.request.account_id != account_id
                || entry.frame.request.source_installation_id != pair.source_installation_id
                || entry.frame.request.target_installation_id != pair.target_installation_id
            {
                continue;
            }
            return match entry.state {
                ActiveRuntimeState::Queued => {
                    entry.state = ActiveRuntimeState::Evaluating { stream_id };
                    NextDrainAction::Evaluate(Box::new(entry.frame.clone()))
                }
                ActiveRuntimeState::Evaluating { .. } | ActiveRuntimeState::InFlight { .. } => {
                    NextDrainAction::WaitForInFlight
                }
            };
        }
        NextDrainAction::Idle
    }

    pub fn revert_evaluating(&mut self, id: &OutboxId, stream_id: u64) {
        let Some(&seq) = self.active_by_id.get(id) else {
            return;
        };
        if let Some(entry) = self.active.get_mut(&seq) {
            if entry.state == (ActiveRuntimeState::Evaluating { stream_id }) {
                entry.state = ActiveRuntimeState::Queued;
            }
        }
    }

    pub fn prepare_claim(
        &mut self,
        pair: &InstallationPair,
        id: &OutboxId,
        stream_id: u64,
        claimed_at_ms: u64,
    ) -> Option<PersistedOutboxRecord> {
        if !self.is_current_stream(pair, stream_id) {
            self.revert_evaluating(id, stream_id);
            return None;
        }
        let seq = *self.active_by_id.get(id)?;
        let entry = self.active.get_mut(&seq)?;
        if entry.state != (ActiveRuntimeState::Evaluating { stream_id }) {
            return None;
        }
        entry.state = ActiveRuntimeState::InFlight {
            stream_id,
            claimed_at_ms,
        };
        Some(entry.to_persisted())
    }

    pub fn revert_unpersisted_claim(&mut self, id: &OutboxId, stream_id: u64) {
        let Some(&seq) = self.active_by_id.get(id) else {
            return;
        };
        if let Some(entry) = self.active.get_mut(&seq) {
            if matches!(
                entry.state,
                ActiveRuntimeState::InFlight {
                    stream_id: active_stream,
                    ..
                } if active_stream == stream_id
            ) {
                entry.state = ActiveRuntimeState::Queued;
            }
        }
    }

    pub fn persisted_queued_copy(&self, id: &OutboxId) -> Option<PersistedOutboxRecord> {
        let seq = *self.active_by_id.get(id)?;
        let entry = self.active.get(&seq)?;
        Some(PersistedOutboxRecord {
            version: PersistedOutboxRecord::CURRENT_VERSION,
            frame: entry.frame.clone(),
            state: PersistedActiveState::Queued,
            accounted_bytes: entry.accounted_bytes,
        })
    }

    pub fn settle_not_written(&mut self, id: &OutboxId, stream_id: u64) -> bool {
        let Some(&seq) = self.active_by_id.get(id) else {
            return false;
        };
        let Some(entry) = self.active.get_mut(&seq) else {
            return false;
        };
        if matches!(
            entry.state,
            ActiveRuntimeState::InFlight {
                stream_id: active_stream,
                ..
            } if active_stream == stream_id
        ) {
            entry.state = ActiveRuntimeState::Queued;
            true
        } else {
            false
        }
    }

    pub fn settle_acked(
        &mut self,
        id: &OutboxId,
        reply_body: Option<Vec<u8>>,
        now_ms: u64,
    ) -> Option<(PersistedSettledReceipt, Vec<PersistedSettledReceipt>)> {
        let entry = self.remove_active(id)?;
        let has_application_reply = reply_body.is_some();
        if let Some(body) = reply_body {
            self.replies.insert(
                id.clone(),
                CompletedApplicationReply {
                    id: id.clone(),
                    completed_at_ms: now_ms,
                    body: Some(body),
                },
            );
        }
        let receipt = PersistedSettledReceipt {
            version: PersistedSettledReceipt::CURRENT_VERSION,
            id: entry.frame.id,
            sequence: entry.frame.sequence,
            status: OutboxRecordStatus::Delivered {
                sequence: entry.frame.sequence,
                delivered_at_ms: now_ms,
                bytes_written: Some(true),
                has_application_reply,
            },
        };
        let evicted = self.insert_settled_bounded(receipt.clone());
        Some((receipt, evicted))
    }

    pub fn settle_unconfirmed(
        &mut self,
        id: &OutboxId,
        reason: UnconfirmedReason,
        bytes_written: Option<bool>,
        detail: Option<String>,
        now_ms: u64,
    ) -> Option<PersistedUnconfirmedReceipt> {
        let entry = self.remove_active(id)?;
        let receipt = PersistedUnconfirmedReceipt {
            version: PersistedUnconfirmedReceipt::CURRENT_VERSION,
            id: entry.frame.id.clone(),
            sequence: entry.frame.sequence,
            account_id: entry.frame.request.account_id,
            source_installation_id: entry.frame.request.source_installation_id,
            target_installation_id: entry.frame.request.target_installation_id,
            payload_digest: entry.frame.payload_digest,
            reason,
            bytes_written: StreamDeliveryOutcome::normalize_unconfirmed_bytes_written(
                bytes_written,
            ),
            detail,
            settled_at_ms: now_ms,
        };
        self.unconfirmed.insert(receipt.id.clone(), receipt.clone());
        Some(receipt)
    }

    pub fn settle_refused_before_write(
        &mut self,
        id: &OutboxId,
        reason: String,
        now_ms: u64,
    ) -> Option<(PersistedSettledReceipt, Vec<PersistedSettledReceipt>)> {
        let entry = self.remove_active(id)?;
        let receipt = PersistedSettledReceipt {
            version: PersistedSettledReceipt::CURRENT_VERSION,
            id: entry.frame.id,
            sequence: entry.frame.sequence,
            status: OutboxRecordStatus::RefusedBeforeWrite {
                sequence: entry.frame.sequence,
                reason,
                bytes_written: Some(false),
                settled_at_ms: now_ms,
            },
        };
        let evicted = self.insert_settled_bounded(receipt.clone());
        Some((receipt, evicted))
    }

    pub fn dismiss_unconfirmed(&mut self, id: &OutboxId) -> Option<PersistedUnconfirmedReceipt> {
        self.unconfirmed.remove(id)
    }

    pub fn record_status(&self, id: &OutboxId) -> Option<OutboxRecordStatus> {
        if let Some(&seq) = self.active_by_id.get(id) {
            if let Some(entry) = self.active.get(&seq) {
                return Some(match entry.state {
                    ActiveRuntimeState::Queued | ActiveRuntimeState::Evaluating { .. } => {
                        OutboxRecordStatus::Queued {
                            sequence: entry.frame.sequence,
                            enqueued_at_ms: entry.frame.enqueued_at_ms,
                            bytes_written: Some(false),
                        }
                    }
                    ActiveRuntimeState::InFlight {
                        stream_id,
                        claimed_at_ms,
                    } => OutboxRecordStatus::InFlight {
                        sequence: entry.frame.sequence,
                        claimed_at_ms,
                        stream_id,
                        bytes_written: None,
                    },
                });
            }
        }
        if let Some(receipt) = self.unconfirmed.get(id) {
            return Some(receipt.into_status());
        }
        self.settled_by_id
            .get(id)
            .map(|receipt| receipt.status.clone())
    }

    pub fn application_reply(&self, id: &OutboxId) -> Option<CompletedApplicationReply> {
        self.replies.get(id).cloned()
    }

    pub fn unconfirmed_records(&self) -> Vec<(OutboxId, OutboxRecordStatus)> {
        self.unconfirmed
            .iter()
            .map(|(id, receipt)| (id.clone(), receipt.into_status()))
            .collect()
    }

    pub fn active_sequence(&self, id: &OutboxId) -> Option<u64> {
        self.active_by_id.get(id).copied()
    }

    pub fn attached_pairs(&self) -> Vec<InstallationPair> {
        self.streams.keys().cloned().collect()
    }

    pub fn has_attached_stream(&self, pair: &InstallationPair) -> bool {
        self.streams.contains_key(pair)
    }

    pub fn snapshot(&self) -> OutboxSnapshot {
        let mut queued_count = 0usize;
        let mut in_flight_count = 0usize;
        for entry in self.active.values() {
            match entry.state {
                ActiveRuntimeState::Queued | ActiveRuntimeState::Evaluating { .. } => {
                    queued_count += 1;
                }
                ActiveRuntimeState::InFlight { .. } => {
                    in_flight_count += 1;
                }
            }
        }
        OutboxSnapshot {
            queued_count,
            in_flight_count,
            active_bytes: self.active_bytes,
            unconfirmed_count: self.unconfirmed.len(),
            attached_stream_count: self.streams.len(),
        }
    }

    fn occupied_count(&self) -> usize {
        self.active.len().saturating_add(self.reserved_count)
    }

    fn occupied_bytes(&self) -> usize {
        self.active_bytes.saturating_add(self.reserved_bytes)
    }

    fn remove_active(&mut self, id: &OutboxId) -> Option<ActiveEntry> {
        let seq = self.active_by_id.remove(id)?;
        let entry = self.active.remove(&seq)?;
        self.active_bytes = self.active_bytes.saturating_sub(entry.accounted_bytes);
        Some(entry)
    }

    fn insert_settled_bounded(
        &mut self,
        receipt: PersistedSettledReceipt,
    ) -> Vec<PersistedSettledReceipt> {
        let mut evicted = Vec::new();
        if self.limits.max_retained_settled == 0 {
            self.replies.remove(&receipt.id);
            return evicted;
        }
        self.settled_order.push_back(receipt.id.clone());
        self.settled_by_id.insert(receipt.id.clone(), receipt);
        while self.settled_order.len() > self.limits.max_retained_settled {
            if let Some(oldest_id) = self.settled_order.pop_front() {
                self.replies.remove(&oldest_id);
                if let Some(removed) = self.settled_by_id.remove(&oldest_id) {
                    evicted.push(removed);
                }
            }
        }
        evicted
    }
}
