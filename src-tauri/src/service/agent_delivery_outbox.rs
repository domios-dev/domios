use super::delivery_error::DeliveryError;
use super::messenger::{AgentDeliveryPayload, Delivery, DeliveryWriteObserver};
use super::pending_delivery_payloads::agent_deliveries::AgentDeliveryRecord;
use super::ServiceContext;
use std::sync::Arc;

enum AgentWriteResult {
    Continue,
    Refused,
}

impl ServiceContext {
    pub(crate) fn deliver_agent_payload(
        &self,
        target: &str,
        mut payload: AgentDeliveryPayload,
        visible_text: &str,
    ) -> Result<Delivery, String> {
        self.pin_agent_delivery_binding(target, &mut payload)?;
        let encoded = serde_json::to_string(&payload)
            .map_err(|_| "agent delivery payload serialization failed")?;
        let idempotency_key = self.agent_delivery_idempotency_key(&payload);
        let owner = Arc::clone(&self.pending_delivery_payloads.agent_deliveries);
        let pty_claim = self.pty_ownership.acquire(target);
        if owner.has_pending(target)? {
            let id = owner.enqueue(target, &encoded, idempotency_key)?;
            drop(pty_claim);
            self.schedule_agent_delivery_flush(target);
            return Ok(Delivery::Pending { delivery_id: id });
        }
        let generation = owner.state_generation(target);
        match self.send_queued_agent_payload(target, &payload) {
            Ok(()) => {
                drop(pty_claim);
                self.record_agent_delivery_written(target, None, &payload, false, "written");
                Ok(Delivery::Pty {
                    text: visible_text.to_string(),
                })
            }
            Err(error)
                if DeliveryError::from_opaque(target, error.clone()).code()
                    == "tab_busy_refused"
                    && self.agent_delivery_binding_matches(target, &payload) =>
            {
                let id = owner.enqueue(target, &encoded, idempotency_key)?;
                let blocked = owner.block_after_refusal(target, generation);
                drop(pty_claim);
                log::warn!(
                    "[agent-delivery] tab={} delivery={} outcome=refused_pending reason=tab_busy_refused",
                    target,
                    id
                );
                if !blocked {
                    self.wake_agent_deliveries_on_state_change(target);
                }
                Ok(Delivery::Pending { delivery_id: id })
            }
            Err(error) => {
                let classified = DeliveryError::from_opaque(target, error.clone());
                drop(pty_claim);
                if classified.bytes_written() {
                    self.record_agent_delivery_written(
                        target,
                        None,
                        &payload,
                        false,
                        "written_submit_unconfirmed",
                    );
                    Ok(Delivery::Pty {
                        text: visible_text.to_string(),
                    })
                } else {
                    Err(error)
                }
            }
        }
    }

    pub(crate) fn wake_agent_deliveries_on_state_change(&self, tab_id: &str) {
        self.pending_delivery_payloads
            .agent_deliveries
            .state_changed(tab_id);
        self.schedule_agent_delivery_flush(tab_id);
    }

    pub(crate) fn schedule_agent_delivery_flush(&self, tab_id: &str) {
        let owner = Arc::clone(&self.pending_delivery_payloads.agent_deliveries);
        if !owner.start_worker(tab_id) {
            return;
        }
        let service = self.clone();
        let tab = tab_id.to_string();
        tauri::async_runtime::spawn_blocking(move || service.flush_agent_deliveries(&tab));
    }

    fn flush_agent_deliveries(&self, tab_id: &str) {
        let owner = Arc::clone(&self.pending_delivery_payloads.agent_deliveries);
        loop {
            let generation = owner.state_generation(tab_id);
            let record = match owner.pending_head(tab_id) {
                Ok(Some(record)) => record,
                Ok(None) => break,
                Err(error) => {
                    log::error!(
                        "[agent-delivery] tab={} outbox read failed: {}",
                        tab_id,
                        error
                    );
                    owner.block_after_refusal(tab_id, generation);
                    break;
                }
            };
            let payload = match owner.payload(tab_id, &record).and_then(|text| {
                serde_json::from_str::<AgentDeliveryPayload>(&text)
                    .map_err(|_| "agent delivery payload decode failed".to_string())
            }) {
                Ok(payload) => payload,
                Err(error) => {
                    log::error!(
                        "[agent-delivery] tab={} delivery={} payload held: {}",
                        tab_id,
                        record.id,
                        error
                    );
                    owner.block_after_refusal(tab_id, generation);
                    break;
                }
            };
            match self.flush_agent_delivery_record(tab_id, &owner, &record, &payload, generation) {
                AgentWriteResult::Continue => continue,
                AgentWriteResult::Refused => break,
            }
        }
        owner.finish_worker(tab_id);
        if owner.pending_head(tab_id).ok().flatten().is_some() {
            self.schedule_agent_delivery_flush(tab_id);
        }
    }

    fn flush_agent_delivery_record(
        &self,
        tab_id: &str,
        owner: &super::pending_delivery_payloads::agent_deliveries::AgentDeliveryStore,
        record: &AgentDeliveryRecord,
        payload: &AgentDeliveryPayload,
        generation: u64,
    ) -> AgentWriteResult {
        if !self.agent_delivery_binding_matches(tab_id, payload) {
            match owner.discard_expired(tab_id, &record.id) {
                Ok(true) => self.expire_agent_delivery(tab_id, &record.id, payload),
                Ok(false) => {}
                Err(error) => {
                    log::error!(
                        "[agent-delivery] tab={} delivery={} stale record cleanup failed: {}",
                        tab_id,
                        record.id,
                        error
                    );
                    return if owner.block_after_refusal(tab_id, generation) {
                        AgentWriteResult::Refused
                    } else {
                        AgentWriteResult::Continue
                    };
                }
            }
            return AgentWriteResult::Continue;
        }
        let pty_claim = self.pty_ownership.acquire(tab_id);
        let claim = match owner.claim(tab_id, &record.id) {
            Ok(claim) => claim,
            Err(error) => {
                log::error!(
                    "[agent-delivery] tab={} delivery={} claim failed: {}",
                    tab_id,
                    record.id,
                    error
                );
                drop(pty_claim);
                return if owner.block_after_refusal(tab_id, generation) {
                    AgentWriteResult::Refused
                } else {
                    AgentWriteResult::Continue
                };
            }
        };
        if !claim {
            drop(pty_claim);
            owner.block_after_refusal(tab_id, generation);
            return AgentWriteResult::Refused;
        }
        let result = self.send_queued_agent_payload(tab_id, payload);
        drop(pty_claim);
        match result {
            Ok(()) => {
                self.settle_written_agent_delivery(tab_id, owner, record, payload, "written");
                AgentWriteResult::Continue
            }
            Err(error) => {
                let classified = DeliveryError::from_opaque(tab_id, error.clone());
                if classified.bytes_written() {
                    self.settle_written_agent_delivery(
                        tab_id,
                        owner,
                        record,
                        payload,
                        "written_submit_unconfirmed",
                    );
                    log::warn!(
                        "[agent-delivery] tab={} delivery={} outcome=written_submit_unconfirmed",
                        tab_id,
                        record.id
                    );
                    AgentWriteResult::Continue
                } else {
                    if !self.agent_delivery_binding_matches(tab_id, payload) {
                        if let Err(cleanup) = owner.discard_expired(tab_id, &record.id) {
                            log::error!(
                                "[agent-delivery] tab={} delivery={} stale record cleanup failed: {}",
                                tab_id,
                                record.id,
                                cleanup
                            );
                            owner.block_after_refusal(tab_id, generation);
                        } else {
                            self.expire_agent_delivery(tab_id, &record.id, payload);
                        }
                        return AgentWriteResult::Continue;
                    }
                    if let Err(storage_error) = owner.release_zero_write(tab_id, &record.id) {
                        log::error!(
                            "[agent-delivery] tab={} delivery={} release failed: {}",
                            tab_id,
                            record.id,
                            storage_error
                        );
                    }
                    log::warn!(
                        "[agent-delivery] tab={} delivery={} outcome=refused_pending reason={}",
                        tab_id,
                        record.id,
                        classified.code()
                    );
                    if !owner.block_after_refusal(tab_id, generation) {
                        AgentWriteResult::Continue
                    } else {
                        AgentWriteResult::Refused
                    }
                }
            }
        }
    }

    fn expire_agent_delivery(
        &self,
        tab_id: &str,
        delivery_id: &str,
        payload: &AgentDeliveryPayload,
    ) {
        if let Some(DeliveryWriteObserver::Report { report_id, .. }) = payload.observer.as_ref() {
            self.report_store.lock().release_delivery(report_id);
        }
        log::warn!(
            "[agent-delivery] tab={} delivery={} outcome=expired_launch",
            tab_id,
            delivery_id
        );
    }

    fn settle_written_agent_delivery(
        &self,
        tab_id: &str,
        owner: &super::pending_delivery_payloads::agent_deliveries::AgentDeliveryStore,
        record: &AgentDeliveryRecord,
        payload: &AgentDeliveryPayload,
        outcome: &str,
    ) {
        match owner.complete_written(tab_id, &record.id) {
            Ok(true) => {}
            Ok(false) => log::warn!(
                "[agent-delivery] tab={} delivery={} settled after write without active record",
                tab_id,
                record.id
            ),
            Err(error) => log::error!(
                "[agent-delivery] tab={} delivery={} write recorded but outbox cleanup failed: {}",
                tab_id,
                record.id,
                error
            ),
        }
        self.record_agent_delivery_written(tab_id, Some(&record.id), payload, true, outcome);
    }

    fn agent_delivery_idempotency_key(&self, payload: &AgentDeliveryPayload) -> Option<String> {
        match &payload.observer {
            Some(DeliveryWriteObserver::Report { report_id, .. }) => {
                Some(format!("report:{report_id}"))
            }
            Some(DeliveryWriteObserver::PlanReminder {
                plan_id,
                tab_id,
                reminder_kind,
                state_key,
                ..
            }) => Some(format!(
                "reminder:{}:{tab_id}:{reminder_kind}:{}",
                plan_id.as_deref().unwrap_or("local"),
                state_key.as_deref().unwrap_or("time")
            )),
            _ => payload
                .idempotency_key
                .as_ref()
                .map(|key| format!("message:{key}")),
        }
    }

    fn record_agent_delivery_written(
        &self,
        tab_id: &str,
        delivery_id: Option<&str>,
        payload: &AgentDeliveryPayload,
        queued: bool,
        outcome: &str,
    ) {
        if let Some(observer) = payload.observer.as_ref() {
            match observer {
                DeliveryWriteObserver::Report {
                    report_id,
                    plan_id,
                    task_id,
                    attempt_id,
                } => {
                    self.report_store.lock().mark_delivered(report_id);
                    if queued {
                        log::info!(
                            "[report] target={} report={} outcome={} delivery={}",
                            tab_id,
                            report_id,
                            outcome,
                            delivery_id.unwrap_or("unknown")
                        );
                    }
                    if let (Some(plan_id), Some(task_id), Some(attempt_id)) = (
                        plan_id.as_deref(),
                        task_id.as_deref(),
                        attempt_id.as_deref(),
                    ) {
                        let report_target_written_at = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis()
                            as u64;
                        self.events.publish_generic(
                            "plan.report-target-written",
                            serde_json::json!({
                                "report_id": report_id,
                                "plan_id": plan_id,
                                "task_id": task_id,
                                "attempt_id": attempt_id,
                                "target_tab_id": tab_id,
                                "report_target_written_at": report_target_written_at,
                            }),
                        );
                        if let Err(error) = self.plan_store.mark_report_target_written(
                            plan_id,
                            task_id,
                            attempt_id,
                            report_target_written_at,
                        ) {
                            log::warn!(
                                "[report] failed to mark report {report_id} written for {plan_id}/{task_id}: {error}"
                            );
                        }
                    }
                }
                DeliveryWriteObserver::PlanRole { plan_id, role } if queued => {
                    log::info!(
                        "[plan-role-delivery] plan={} role={} intended_target={} actual_target={} outcome={} delivery_id={}",
                        plan_id,
                        role,
                        tab_id,
                        tab_id,
                        outcome,
                        delivery_id.unwrap_or("unknown")
                    );
                }
                DeliveryWriteObserver::PlanReminder {
                    plan_id,
                    tab_id: reminder_tab,
                    reminder_kind,
                    state_key,
                    complete_wrapup_plan_id,
                } if queued => {
                    let written_at = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    if reminder_kind != "mode_change" {
                        self.plan_turn_counter
                            .mark_time_nudged(reminder_tab, written_at);
                    }
                    if !matches!(reminder_kind.as_str(), "mode_change" | "complete_wrapup") {
                        self.plan_turn_counter
                            .record_time_delivery(reminder_tab, written_at);
                        if let Some(state_key) = state_key {
                            self.plan_turn_counter.mark_reminder_content_delivered(
                                reminder_tab,
                                state_key.clone(),
                                written_at,
                            );
                        }
                    }
                    if let Some(plan_id) = complete_wrapup_plan_id {
                        self.plan_turn_counter.mark_complete_wrapup_sent(plan_id);
                    }
                    log::info!(
                        "[plan-reminder] written {:?} nudge to Tab {} plan={}",
                        reminder_kind,
                        reminder_tab,
                        plan_id.as_deref().unwrap_or("local")
                    );
                }
                _ => {}
            }
        }
        if queued {
            log::info!(
                "[agent-delivery] tab={} delivery={} outcome={}",
                tab_id,
                delivery_id.unwrap_or("unknown"),
                outcome
            );
        }
    }
}
