use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

use crate::mesh::crown::{AuthenticatedAgent, AuthenticatedOrigin};
use crate::service::chat_send_transaction::{ChatSendState, ChatSendStatus};

use super::types::{CloudError, CLOUD_WAKE_SEND_PREFIX};

pub const DEFAULT_WAKE_HOLD_DEADLINE_MS: u64 = 180_000;
pub const MIN_WAKE_RETRY_READINESS_BUDGET_MS: u64 = 15_000;
pub const MAX_HELD_SENDS_PER_MACHINE: usize = 32;
pub const RELEASE_CONDITION_PEER_READY: &str = "peer_ready";
pub const EXPIRED_NOT_DELIVERED_CODE: &str = "expired_not_delivered";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeldCloudSendState {
    Held,
    Claimed,
    Written,
    Delivered,
    SubmitUnconfirmed,
    ExpiredNotDelivered,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HeldCloudSend {
    pub send_id: String,
    pub sequence: u64,
    pub machine_id: String,
    pub tab_id: String,
    pub delivery_id: String,
    pub method: String,
    pub path: String,
    pub body: serde_json::Value,
    pub origin: AuthenticatedOrigin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AuthenticatedAgent>,
    pub admitted_at_ms: u64,
    pub deadline_ms: u64,
    pub release_condition: String,
    pub state: HeldCloudSendState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl HeldCloudSend {
    fn mark_expired(&mut self) {
        self.state = HeldCloudSendState::ExpiredNotDelivered;
        self.error = Some(format!(
            "{EXPIRED_NOT_DELIVERED_CODE}: cloud machine {} did not connect before the wake deadline; nothing was written",
            self.machine_id
        ));
    }

    pub fn to_chat_send_status(&self) -> ChatSendStatus {
        let state = match self.state {
            HeldCloudSendState::Held => ChatSendState::Pending,
            HeldCloudSendState::Claimed | HeldCloudSendState::SubmitUnconfirmed => {
                ChatSendState::Unknown
            }
            HeldCloudSendState::Written => ChatSendState::Written,
            HeldCloudSendState::Delivered => ChatSendState::Sent,
            HeldCloudSendState::ExpiredNotDelivered
            | HeldCloudSendState::Failed
            | HeldCloudSendState::Cancelled => ChatSendState::Failed,
        };
        let pending_reason = (self.state == HeldCloudSendState::Held).then(|| {
            format!(
                "Waiting for cloud machine {} ({}; deadline_ms={}). Cancel before write with domios tab send-cancel {}.",
                self.machine_id, self.release_condition, self.deadline_ms, self.send_id
            )
        });
        let correlation_id = self
            .body
            .get("client_token")
            .and_then(serde_json::Value::as_str)
            .filter(|token| !token.is_empty())
            .map(str::to_owned);
        ChatSendStatus {
            send_id: self.send_id.clone(),
            tab_id: self.tab_id.clone(),
            state,
            error: self.error.clone(),
            ledger_id: "cloud-wake-hold".into(),
            sequence: self.sequence,
            retained_from: self.sequence,
            correlation_id,
            matched_row: None,
            pending_reason,
            system_send: None,
        }
    }
}

/// Pure input describing mesh state for a cloud machine's guest link readiness.
/// A persisted cloud endpoint alone never implies readiness.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuestLinkReadinessInput {
    pub traffic_open: bool,
    pub auth_refused: bool,
    pub installation_unavailable: bool,
    pub stream_attached: bool,
    pub stream_binding_token: Option<String>,
    pub peer_online: bool,
    pub link_nonce: Option<String>,
    pub has_persisted_endpoint: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuestLinkReadiness {
    pub ready: bool,
    pub link_generation: Option<String>,
}

/// Evaluates whether a cloud machine guest is genuinely linked and ready to receive sends.
/// Branches strictly on mesh link/stream state; a persisted endpoint alone is never ready.
pub fn evaluate_guest_link_readiness(input: &GuestLinkReadinessInput) -> GuestLinkReadiness {
    if !input.traffic_open || input.auth_refused || input.installation_unavailable {
        return GuestLinkReadiness {
            ready: false,
            link_generation: None,
        };
    }
    if input.stream_attached {
        let token = input
            .stream_binding_token
            .as_deref()
            .filter(|t| !t.is_empty())
            .unwrap_or("attached");
        return GuestLinkReadiness {
            ready: true,
            link_generation: Some(format!("stream:{token}")),
        };
    }
    if input.peer_online {
        let generation = match input.link_nonce.as_deref().filter(|n| !n.is_empty()) {
            Some(nonce) => format!("link:{nonce}"),
            None => "online".to_owned(),
        };
        return GuestLinkReadiness {
            ready: true,
            link_generation: Some(generation),
        };
    }
    GuestLinkReadiness {
        ready: false,
        link_generation: None,
    }
}

/// Tracks the last flushed link generation per machine so a given guest link/online generation
/// triggers at most one flush attempt.
#[derive(Default)]
pub struct LinkGenerationFlushTracker {
    last_flushed_generation: HashMap<String, String>,
}

impl LinkGenerationFlushTracker {
    pub fn should_flush(&mut self, machine_id: &str, generation: &str) -> bool {
        if generation.is_empty() {
            return false;
        }
        if self
            .last_flushed_generation
            .get(machine_id)
            .map(String::as_str)
            == Some(generation)
        {
            return false;
        }
        self.last_flushed_generation
            .insert(machine_id.to_owned(), generation.to_owned());
        true
    }

    pub fn clear(&mut self, machine_id: &str) {
        self.last_flushed_generation.remove(machine_id);
    }
}

/// Returns true when a wake provisioning error should be retried once with the same `request_id`
/// while enough time remains inside the overall wake deadline for retry and guest readiness.
pub fn should_retry_wake_once(
    error: &CloudError,
    attempt: u8,
    now_ms: u64,
    wake_deadline_ms: u64,
) -> bool {
    attempt == 0
        && error.is_retryable_wake_error()
        && wake_deadline_ms.saturating_sub(now_ms) >= MIN_WAKE_RETRY_READINESS_BUDGET_MS
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FlushWriteOutcome {
    Written,
    Delivered,
    RefusedMachineSleeping,
    RefusedPreWrite(String),
    Uncertain(String),
}

#[derive(Default)]
pub struct CloudWakeQueue {
    next_sequence: u64,
    by_id: HashMap<String, HeldCloudSend>,
    order_by_machine: HashMap<String, VecDeque<String>>,
}

impl CloudWakeQueue {
    pub fn is_cloud_wake_send_id(send_id: &str) -> bool {
        send_id.starts_with(CLOUD_WAKE_SEND_PREFIX)
    }

    /// Reconstruct queue from persisted state. Any send that was `Claimed` before restart
    /// becomes `SubmitUnconfirmed` and is never replayed.
    pub fn from_persisted(next_sequence: u64, mut sends: Vec<HeldCloudSend>) -> (Self, bool) {
        sends.sort_by_key(|rec| rec.sequence);
        let mut max_seq = next_sequence;
        let mut reconciled_claimed = false;
        let mut by_id = HashMap::with_capacity(sends.len());
        let mut order_by_machine: HashMap<String, VecDeque<String>> = HashMap::new();
        for mut rec in sends {
            max_seq = max_seq.max(rec.sequence);
            if rec.state == HeldCloudSendState::Claimed {
                rec.state = HeldCloudSendState::SubmitUnconfirmed;
                rec.error = Some(
                    "submit_unconfirmed: process restarted after claim before outcome was recorded"
                        .into(),
                );
                reconciled_claimed = true;
            }
            order_by_machine
                .entry(rec.machine_id.clone())
                .or_default()
                .push_back(rec.send_id.clone());
            by_id.insert(rec.send_id.clone(), rec);
        }
        (
            Self {
                next_sequence: max_seq,
                by_id,
                order_by_machine,
            },
            reconciled_claimed,
        )
    }

    pub fn snapshot_persisted(&self) -> (u64, Vec<HeldCloudSend>) {
        let mut sends: Vec<HeldCloudSend> = self.by_id.values().cloned().collect();
        sends.sort_by_key(|rec| rec.sequence);
        (self.next_sequence, sends)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn admit(
        &mut self,
        machine_id: &str,
        tab_id: &str,
        delivery_id: String,
        method: String,
        path: String,
        body: serde_json::Value,
        origin: AuthenticatedOrigin,
        agent: Option<AuthenticatedAgent>,
        now_ms: u64,
        deadline_ms: u64,
    ) -> Result<HeldCloudSend, CloudError> {
        let queue = self
            .order_by_machine
            .entry(machine_id.to_owned())
            .or_default();
        let active_held = queue
            .iter()
            .filter_map(|id| self.by_id.get(id))
            .filter(|rec| {
                matches!(
                    rec.state,
                    HeldCloudSendState::Held | HeldCloudSendState::Claimed
                )
            })
            .count();
        if active_held >= MAX_HELD_SENDS_PER_MACHINE {
            return Err(CloudError::Internal(format!(
                "held send queue full for cloud machine {machine_id} (max {MAX_HELD_SENDS_PER_MACHINE})"
            )));
        }

        // Deduplicate by delivery_id so a retried relay cannot enqueue the same payload twice.
        if let Some(existing) = self
            .by_id
            .values()
            .find(|rec| rec.machine_id == machine_id && rec.delivery_id == delivery_id)
        {
            return Ok(existing.clone());
        }

        self.next_sequence = self.next_sequence.saturating_add(1);
        let send_id = format!("{CLOUD_WAKE_SEND_PREFIX}{}", uuid::Uuid::new_v4());
        let record = HeldCloudSend {
            send_id: send_id.clone(),
            sequence: self.next_sequence,
            machine_id: machine_id.to_owned(),
            tab_id: tab_id.to_owned(),
            delivery_id,
            method,
            path,
            body,
            origin,
            agent,
            admitted_at_ms: now_ms,
            deadline_ms,
            release_condition: RELEASE_CONDITION_PEER_READY.into(),
            state: HeldCloudSendState::Held,
            error: None,
        };
        queue.push_back(send_id.clone());
        self.by_id.insert(send_id, record.clone());
        Ok(record)
    }

    /// Claim the next never-written held send in FIFO order before any write attempt.
    pub fn claim_next_for_machine(
        &mut self,
        machine_id: &str,
        now_ms: u64,
    ) -> Option<HeldCloudSend> {
        let queue = self.order_by_machine.get(machine_id)?;
        for id in queue {
            if let Some(rec) = self.by_id.get_mut(id) {
                if rec.state == HeldCloudSendState::Held {
                    if now_ms >= rec.deadline_ms {
                        rec.mark_expired();
                        continue;
                    }
                    rec.state = HeldCloudSendState::Claimed;
                    return Some(rec.clone());
                }
            }
        }
        None
    }

    /// Reconcile a claimed send's transport outcome. Never replays written or uncertain sends.
    pub fn settle_claimed(
        &mut self,
        send_id: &str,
        outcome: FlushWriteOutcome,
    ) -> Option<HeldCloudSend> {
        let rec = self.by_id.get_mut(send_id)?;
        if rec.state != HeldCloudSendState::Claimed {
            return Some(rec.clone());
        }
        match outcome {
            FlushWriteOutcome::Written => {
                rec.state = HeldCloudSendState::Written;
                rec.error = None;
            }
            FlushWriteOutcome::Delivered => {
                rec.state = HeldCloudSendState::Delivered;
                rec.error = None;
            }
            FlushWriteOutcome::RefusedMachineSleeping => {
                // Proven bytes_written=false before write (sleeping or post-wake ingress unavailable):
                // safe to keep held for next wake/connect.
                rec.state = HeldCloudSendState::Held;
                rec.error = None;
            }
            FlushWriteOutcome::RefusedPreWrite(reason) => {
                rec.state = HeldCloudSendState::Failed;
                rec.error = Some(reason);
            }
            FlushWriteOutcome::Uncertain(detail) => {
                rec.state = HeldCloudSendState::SubmitUnconfirmed;
                rec.error = Some(format!("submit_unconfirmed: {detail}"));
            }
        }
        Some(rec.clone())
    }

    pub fn expire_overdue(&mut self, machine_id: &str, now_ms: u64) -> Vec<HeldCloudSend> {
        let mut expired = Vec::new();
        let Some(queue) = self.order_by_machine.get(machine_id) else {
            return expired;
        };
        for id in queue {
            if let Some(rec) = self.by_id.get_mut(id) {
                if rec.state == HeldCloudSendState::Held && now_ms >= rec.deadline_ms {
                    rec.mark_expired();
                    expired.push(rec.clone());
                }
            }
        }
        expired
    }

    pub fn expire_all_overdue(&mut self, now_ms: u64) -> Vec<HeldCloudSend> {
        let machine_ids: Vec<String> = self.order_by_machine.keys().cloned().collect();
        let mut expired = Vec::new();
        for machine_id in machine_ids {
            expired.extend(self.expire_overdue(&machine_id, now_ms));
        }
        expired
    }

    pub fn earliest_held_deadline_ms(&self) -> Option<u64> {
        self.by_id
            .values()
            .filter(|rec| rec.state == HeldCloudSendState::Held)
            .map(|rec| rec.deadline_ms)
            .min()
    }

    pub fn machines_with_held_sends(&self) -> Vec<String> {
        let mut machines: Vec<String> = self
            .order_by_machine
            .iter()
            .filter(|(_, queue)| {
                queue.iter().any(|id| {
                    self.by_id
                        .get(id)
                        .is_some_and(|rec| rec.state == HeldCloudSendState::Held)
                })
            })
            .map(|(machine_id, _)| machine_id.clone())
            .collect();
        machines.sort();
        machines
    }

    pub fn cancel_before_write_at(
        &mut self,
        send_id: &str,
        now_ms: u64,
    ) -> Result<HeldCloudSend, CloudError> {
        let rec = self
            .by_id
            .get_mut(send_id)
            .ok_or_else(|| CloudError::MachineNotFound(format!("retained send {send_id}")))?;
        if rec.state == HeldCloudSendState::Held && now_ms >= rec.deadline_ms {
            rec.mark_expired();
            return Ok(rec.clone());
        }
        if matches!(
            rec.state,
            HeldCloudSendState::Cancelled | HeldCloudSendState::ExpiredNotDelivered
        ) {
            return Ok(rec.clone());
        }
        if rec.state != HeldCloudSendState::Held {
            return Err(CloudError::SendAlreadyClaimed(
                "send cannot be cancelled: payload may already have been written; inspect send-status and never replay uncertain input".into(),
            ));
        }
        rec.state = HeldCloudSendState::Cancelled;
        rec.error = Some("cancelled_before_write".into());
        Ok(rec.clone())
    }

    pub fn cancel_before_write(&mut self, send_id: &str) -> Result<HeldCloudSend, CloudError> {
        self.cancel_before_write_at(send_id, 0)
    }

    pub fn inspect(&self, send_id: &str) -> Option<ChatSendStatus> {
        self.by_id
            .get(send_id)
            .map(HeldCloudSend::to_chat_send_status)
    }

    /// Inspects a send at `now_ms`, transitioning an overdue `Held` send to `ExpiredNotDelivered`
    /// so inspection past the deadline never reports a stranded `held` state.
    pub fn inspect_at(
        &mut self,
        send_id: &str,
        now_ms: u64,
    ) -> (Option<ChatSendStatus>, Option<HeldCloudSend>) {
        let Some(rec) = self.by_id.get_mut(send_id) else {
            return (None, None);
        };
        let expired_now = if rec.state == HeldCloudSendState::Held && now_ms >= rec.deadline_ms {
            rec.mark_expired();
            Some(rec.clone())
        } else {
            None
        };
        (Some(rec.to_chat_send_status()), expired_now)
    }

    pub fn held_count_for_machine(&self, machine_id: &str) -> usize {
        self.order_by_machine
            .get(machine_id)
            .map(|queue| {
                queue
                    .iter()
                    .filter_map(|id| self.by_id.get(id))
                    .filter(|rec| rec.state == HeldCloudSendState::Held)
                    .count()
            })
            .unwrap_or(0)
    }

    pub fn total_held(&self) -> usize {
        self.by_id
            .values()
            .filter(|rec| rec.state == HeldCloudSendState::Held)
            .count()
    }
}

pub struct InFlightWake {
    notify: Notify,
    result: Mutex<Option<Result<(), CloudError>>>,
}

impl InFlightWake {
    pub async fn wait(&self) -> Result<(), CloudError> {
        loop {
            let notified = self.notify.notified();
            if let Some(outcome) = self.result.lock().clone() {
                return outcome;
            }
            notified.await;
        }
    }
}

#[derive(Default)]
pub struct SingleFlightWakes {
    in_flight: Mutex<HashMap<String, Arc<InFlightWake>>>,
}

pub enum WakeSlot {
    Leader(WakeLeaderGuard),
    Follower(Arc<InFlightWake>),
}

struct WakeEntry {
    machine_id: String,
    wake: Arc<InFlightWake>,
}

pub struct WakeLeaderGuard {
    owner: Arc<SingleFlightWakes>,
    entry: Option<WakeEntry>,
}

impl WakeLeaderGuard {
    pub fn finish(mut self, outcome: Result<(), CloudError>) {
        if let Some(entry) = self.entry.take() {
            *entry.wake.result.lock() = Some(outcome);
            self.owner.in_flight.lock().remove(&entry.machine_id);
            entry.wake.notify.notify_waiters();
        }
    }
}

impl Drop for WakeLeaderGuard {
    fn drop(&mut self) {
        if let Some(entry) = self.entry.take() {
            let mut result = entry.wake.result.lock();
            if result.is_none() {
                *result = Some(Err(CloudError::ProviderUnavailable("wake cancelled".into())));
            }
            drop(result);
            self.owner.in_flight.lock().remove(&entry.machine_id);
            entry.wake.notify.notify_waiters();
        }
    }
}

impl SingleFlightWakes {
    pub fn acquire(self: &Arc<Self>, machine_id: &str) -> WakeSlot {
        let mut map = self.in_flight.lock();
        if let Some(existing) = map.get(machine_id) {
            return WakeSlot::Follower(Arc::clone(existing));
        }
        let wake = Arc::new(InFlightWake {
            notify: Notify::new(),
            result: Mutex::new(None),
        });
        map.insert(machine_id.to_owned(), Arc::clone(&wake));
        WakeSlot::Leader(WakeLeaderGuard {
            owner: Arc::clone(self),
            entry: Some(WakeEntry {
                machine_id: machine_id.to_owned(),
                wake,
            }),
        })
    }
}
