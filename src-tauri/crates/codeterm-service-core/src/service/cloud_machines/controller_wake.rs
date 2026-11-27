use crate::mesh::crown::{AuthenticatedAgent, AuthenticatedOrigin};
use crate::service::chat_send_transaction::ChatSendStatus;

use super::controller::CloudMachineController;
use super::held_sends::{FlushWriteOutcome, HeldCloudSend};
use super::ports::CloudPeerControlPort;
use super::state::{transition, LifecycleEvent};
use super::types::{CloudEndpoint, CloudError, CloudLifecycleState, CloudMachineRecord};

pub(super) const WAKE_UNVERIFIED: &str = "wake_peer_unverified: ";

impl CloudMachineController {
    pub(super) fn settle_unverified_wake(
        &self,
        record: &CloudMachineRecord,
        error: &CloudError,
        direct_endpoint: Option<CloudEndpoint>,
    ) {
        let _ = self.update_record(&record.machine_id, |current| {
            let now = self.clock.now_ms();
            if matches!(current.endpoint, Some(CloudEndpoint::ProviderProxy { .. })) {
                let mut awake = transition(current, LifecycleEvent::WakeReady, now)?;
                awake.last_error = Some(format!("{WAKE_UNVERIFIED}{error}"));
                Ok(awake)
            } else {
                let mut failed =
                    transition(current, LifecycleEvent::WakeFailed(error.to_string()), now)?;
                failed.endpoint = direct_endpoint;
                Ok(failed)
            }
        });
    }

    pub fn observe_verified_wake_routes(&self, attached: &std::collections::HashSet<String>) {
        for record in self.list_records() {
            if record.state != CloudLifecycleState::Awake
                || !record
                    .installation_id
                    .as_ref()
                    .is_some_and(|id| attached.contains(id))
                || !record
                    .last_error
                    .as_ref()
                    .is_some_and(|error| error.starts_with(WAKE_UNVERIFIED))
            {
                continue;
            }
            let _ = self.update_record(&record.machine_id, |current| {
                let mut verified = current.clone();
                if current.state == CloudLifecycleState::Awake
                    && current.installation_id == record.installation_id
                    && current.last_error == record.last_error
                {
                    verified.last_error = None;
                }
                Ok(verified)
            });
        }
    }

    pub fn subscribe_wake_queue(&self) -> tokio::sync::watch::Receiver<u64> {
        self.wake_queue_tx.subscribe()
    }

    pub(super) fn notify_wake_queue(&self) {
        self.wake_queue_tx
            .send_modify(|rev| *rev = rev.wrapping_add(1));
    }

    pub(super) fn clear_link_flush_generation(&self, machine_id: &str) {
        self.link_flush_tracker.lock().clear(machine_id);
    }

    fn emit_held_send_status(&self, send: &HeldCloudSend) {
        let observer = self.send_status_observer.lock().clone();
        let Some(observer) = observer else {
            return;
        };
        let record = self.find_by_peer(&send.machine_id);
        observer(record.as_ref(), send);
    }

    pub fn earliest_held_send_deadline_ms(&self) -> Option<u64> {
        self.wake_queue.lock().earliest_held_deadline_ms()
    }

    pub fn machines_with_held_sends(&self) -> Vec<String> {
        self.wake_queue.lock().machines_with_held_sends()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn hold_send_for_wake(
        &self,
        machine_id: &str,
        tab_id: &str,
        delivery_id: String,
        method: String,
        path: String,
        body: serde_json::Value,
        origin: AuthenticatedOrigin,
        agent: Option<AuthenticatedAgent>,
        hold_timeout_ms: u64,
    ) -> Result<HeldCloudSend, CloudError> {
        let now_ms = self.clock.now_ms();
        let deadline_ms = now_ms.saturating_add(hold_timeout_ms);
        let held = {
            let mut queue = self.wake_queue.lock();
            let held = queue.admit(
                machine_id,
                tab_id,
                delivery_id,
                method,
                path,
                body,
                origin,
                agent,
                now_ms,
                deadline_ms,
            )?;
            self.persist_wake_queue_locked(&queue)?;
            held
        };
        self.clear_link_flush_generation(machine_id);
        self.notify_wake_queue();
        Ok(held)
    }

    pub async fn flush_held_sends(
        &self,
        machine_id: &str,
        peer_control: &dyn CloudPeerControlPort,
    ) -> Vec<HeldCloudSend> {
        let flush_lock = self.machine_flush_lock(machine_id);
        let _flush_guard = flush_lock.lock().await;
        let mut settled = self.expire_overdue_held_sends(machine_id);
        settled.extend(
            self.flush_held_sends_after_expiry_locked(machine_id, peer_control)
                .await,
        );
        settled
    }

    /// Re-attempts flushing held sends for `machine_id` on a guest link/online event,
    /// at most once per `link_generation`. Overdue sends expire honestly and are never written.
    pub async fn flush_held_sends_for_link_generation(
        &self,
        machine_id: &str,
        link_generation: &str,
        peer_control: &dyn CloudPeerControlPort,
    ) -> Vec<HeldCloudSend> {
        let flush_lock = self.machine_flush_lock(machine_id);
        let _flush_guard = flush_lock.lock().await;

        let expired = self.expire_overdue_held_sends(machine_id);
        let has_held = self.wake_queue.lock().held_count_for_machine(machine_id) > 0;
        if !has_held {
            return expired;
        }
        if !self
            .link_flush_tracker
            .lock()
            .should_flush(machine_id, link_generation)
        {
            return expired;
        }
        let mut settled = expired;
        settled.extend(
            self.flush_held_sends_after_expiry_locked(machine_id, peer_control)
                .await,
        );
        settled
    }

    async fn flush_held_sends_after_expiry_locked(
        &self,
        machine_id: &str,
        peer_control: &dyn CloudPeerControlPort,
    ) -> Vec<HeldCloudSend> {
        let Ok(record) = self.find_record(machine_id) else {
            return Vec::new();
        };
        let mut settled = Vec::new();
        loop {
            let next = {
                let now_ms = self.clock.now_ms();
                let mut queue = self.wake_queue.lock();
                let claimed = queue.claim_next_for_machine(machine_id, now_ms);
                if claimed.is_some() {
                    // Persist Claimed before writing so restart promotes it to SubmitUnconfirmed, never replaying.
                    if let Err(error) = self.persist_wake_queue_locked(&queue) {
                        log::warn!("[cloud] failed to persist claimed send before flush: {error}");
                        if let Some(ref c) = claimed {
                            let _ = queue.settle_claimed(
                                &c.send_id,
                                FlushWriteOutcome::RefusedMachineSleeping,
                            );
                        }
                        break;
                    }
                }
                claimed
            };
            let Some(claimed) = next else {
                break;
            };
            let outcome = peer_control.flush_send(&record, &claimed).await;
            let keep_held = outcome == FlushWriteOutcome::RefusedMachineSleeping;
            let updated = {
                let mut queue = self.wake_queue.lock();
                let updated = queue.settle_claimed(&claimed.send_id, outcome);
                if let Err(error) = self.persist_wake_queue_locked(&queue) {
                    log::warn!("[cloud] failed to persist settled send outcome: {error}");
                }
                updated
            };
            if let Some(updated) = updated {
                self.notify_wake_queue();
                if !keep_held {
                    self.emit_held_send_status(&updated);
                }
                settled.push(updated);
            }
            if keep_held {
                break;
            }
            settled.extend(self.expire_overdue_held_sends(machine_id));
        }
        settled
    }

    pub fn inspect_held_send(&self, send_id: &str) -> Option<ChatSendStatus> {
        let now_ms = self.clock.now_ms();
        let (status, expired_now) = {
            let mut queue = self.wake_queue.lock();
            let (status, expired_now) = queue.inspect_at(send_id, now_ms);
            if expired_now.is_some() {
                if let Err(error) = self.persist_wake_queue_locked(&queue) {
                    log::warn!("[cloud] failed to persist expired held send: {error}");
                }
            }
            (status, expired_now)
        };
        if let Some(expired) = expired_now {
            self.notify_wake_queue();
            self.emit_held_send_status(&expired);
        }
        status
    }

    pub fn cancel_held_send(&self, send_id: &str) -> Result<ChatSendStatus, CloudError> {
        let now_ms = self.clock.now_ms();
        let rec = {
            let mut queue = self.wake_queue.lock();
            let rec = queue.cancel_before_write_at(send_id, now_ms)?;
            self.persist_wake_queue_locked(&queue)?;
            rec
        };
        self.notify_wake_queue();
        self.emit_held_send_status(&rec);
        Ok(rec.to_chat_send_status())
    }

    pub fn held_send_count(&self) -> usize {
        self.wake_queue.lock().total_held()
    }

    pub fn expire_overdue_held_sends(&self, machine_id: &str) -> Vec<HeldCloudSend> {
        let now_ms = self.clock.now_ms();
        let expired = {
            let mut queue = self.wake_queue.lock();
            let expired = queue.expire_overdue(machine_id, now_ms);
            if !expired.is_empty() {
                let _ = self.persist_wake_queue_locked(&queue);
            }
            expired
        };
        if !expired.is_empty() {
            self.notify_wake_queue();
            for rec in &expired {
                self.emit_held_send_status(rec);
            }
        }
        expired
    }

    pub fn expire_all_overdue_held_sends(&self) -> Vec<HeldCloudSend> {
        let now_ms = self.clock.now_ms();
        let expired = {
            let mut queue = self.wake_queue.lock();
            let expired = queue.expire_all_overdue(now_ms);
            if !expired.is_empty() {
                let _ = self.persist_wake_queue_locked(&queue);
            }
            expired
        };
        if !expired.is_empty() {
            self.notify_wake_queue();
            for rec in &expired {
                self.emit_held_send_status(rec);
            }
        }
        expired
    }
}
