use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

use crate::service::chat_send_transaction::{ChatSendState, ChatSendStatus};

use super::types::{CloudError, CLOUD_WAKE_SEND_PREFIX};

pub const DEFAULT_WAKE_HOLD_DEADLINE_MS: u64 = 180_000;
pub const MAX_HELD_SENDS_PER_MACHINE: usize = 32;
pub const RELEASE_CONDITION_PEER_READY: &str = "peer_ready";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeldCloudSendState {
    Held,
    Claimed,
    Written,
    Delivered,
    SubmitUnconfirmed,
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
    pub admitted_at_ms: u64,
    pub deadline_ms: u64,
    pub release_condition: String,
    pub state: HeldCloudSendState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl HeldCloudSend {
    pub fn to_chat_send_status(&self) -> ChatSendStatus {
        let state = match self.state {
            HeldCloudSendState::Held => ChatSendState::Pending,
            HeldCloudSendState::Claimed | HeldCloudSendState::SubmitUnconfirmed => {
                ChatSendState::Unknown
            }
            HeldCloudSendState::Written => ChatSendState::Written,
            HeldCloudSendState::Delivered => ChatSendState::Sent,
            HeldCloudSendState::Failed | HeldCloudSendState::Cancelled => ChatSendState::Failed,
        };
        let pending_reason = (self.state == HeldCloudSendState::Held).then(|| {
            format!(
                "Waiting for cloud machine {} ({}; deadline_ms={}). Cancel before write with domios tab send-cancel {}.",
                self.machine_id, self.release_condition, self.deadline_ms, self.send_id
            )
        });
        ChatSendStatus {
            send_id: self.send_id.clone(),
            tab_id: self.tab_id.clone(),
            state,
            error: self.error.clone(),
            ledger_id: "cloud-wake-hold".into(),
            sequence: self.sequence,
            retained_from: self.sequence,
            correlation_id: None,
            matched_row: None,
            pending_reason,
            system_send: None,
        }
    }
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

    pub fn admit(
        &mut self,
        machine_id: &str,
        tab_id: &str,
        delivery_id: String,
        method: String,
        path: String,
        body: serde_json::Value,
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
                        rec.state = HeldCloudSendState::Failed;
                        rec.error = Some("wake_deadline_exceeded".into());
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
                // Proven bytes_written=false before write: safe to keep held for next wake.
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
                    rec.state = HeldCloudSendState::Failed;
                    rec.error = Some("wake_deadline_exceeded".into());
                    expired.push(rec.clone());
                }
            }
        }
        expired
    }

    pub fn cancel_before_write(&mut self, send_id: &str) -> Result<HeldCloudSend, CloudError> {
        let rec = self
            .by_id
            .get_mut(send_id)
            .ok_or_else(|| CloudError::MachineNotFound(format!("retained send {send_id}")))?;
        if rec.state == HeldCloudSendState::Cancelled {
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

    pub fn inspect(&self, send_id: &str) -> Option<ChatSendStatus> {
        self.by_id
            .get(send_id)
            .map(HeldCloudSend::to_chat_send_status)
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
    result: Mutex<Option<Result<(), String>>>,
}

impl InFlightWake {
    pub async fn wait(&self) -> Result<(), String> {
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
    pub fn finish(mut self, outcome: Result<(), String>) {
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
                *result = Some(Err("wake cancelled".into()));
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

    pub fn is_waking(&self, machine_id: &str) -> bool {
        self.in_flight.lock().contains_key(machine_id)
    }
}
