use super::delivery_error::DeliveryError;
use super::messenger::{AgentDeliveryPayload, Delivery, DeliveryWriteObserver};
use super::pending_delivery_payloads::agent_deliveries::{
    AgentDeliveryReceipt, AgentDeliveryReceiptStatus, AgentDeliveryRecord, AgentDeliveryRecovery,
};
use super::ServiceContext;
use std::sync::Arc;

enum AgentWriteResult {
    Continue,
    Refused,
}

impl ServiceContext {
    pub fn deliver_agent_payload(
        &self,
        target: &str,
        mut payload: AgentDeliveryPayload,
        visible_text: &str,
    ) -> Result<Delivery, String> {
        let resolved = self
            .mux
            .read()
            .resolve_tab_id(target)
            .ok_or_else(|| format!("Tab {target} not found"))?;
        let target = resolved.as_str();
        if crate::paths::is_dev_instance() && payload.trace_id.is_none() {
            let id = crate::flow_trace::tab_flow(self, target)
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            crate::flow_trace::record(&id, "message.started", target, "", "accepted", None);
            payload.trace_id = Some(id);
        }
        let pty_claim = self.pty_ownership.acquire(target);
        self.pin_agent_delivery_binding(&pty_claim, &mut payload)?;
        let encoded = serde_json::to_string(&payload)
            .map_err(|_| "agent delivery payload serialization failed")?;
        let idempotency_key = self.agent_delivery_idempotency_key(&payload);
        let owner = Arc::clone(&self.pending_delivery_payloads.agent_deliveries);
        if owner.has_pending(target)? {
            let id = owner.enqueue(target, &encoded, idempotency_key)?;
            if let Some(flow) = payload.trace_id.as_deref() {
                crate::flow_trace::record(flow, "delivery.queued", target, "", "held", Some(&id));
            }
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
                if let Some(flow) = payload.trace_id.as_deref() {
                    crate::flow_trace::record(
                        flow,
                        "delivery.queued",
                        target,
                        "",
                        "held",
                        Some(&id),
                    );
                }
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

    pub fn schedule_restored_agent_delivery_reconciliation(&self) {
        let tab_ids = self
            .mux
            .read()
            .snapshot()
            .tabs
            .into_values()
            .map(|tab| tab.id)
            .collect::<Vec<_>>();
        for tab_id in tab_ids {
            let owner = Arc::clone(&self.pending_delivery_payloads.agent_deliveries);
            if !owner.start_worker(&tab_id) {
                continue;
            }
            let service = self.clone();
            tauri::async_runtime::spawn_blocking(move || {
                service.reconcile_retired_agent_deliveries(&tab_id, &owner);
                owner.finish_worker(&tab_id);
                let bound_pending = owner
                    .pending_head(&tab_id)
                    .ok()
                    .flatten()
                    .and_then(|record| {
                        owner.payload(&tab_id, &record).ok().and_then(|text| {
                            serde_json::from_str::<AgentDeliveryPayload>(&text).ok()
                        })
                    })
                    .is_some_and(|payload| {
                        service.agent_delivery_binding_matches(&tab_id, &payload)
                    });
                if bound_pending {
                    service.schedule_agent_delivery_flush(&tab_id);
                }
            });
        }
    }

    fn flush_agent_deliveries(&self, tab_id: &str) {
        let owner = Arc::clone(&self.pending_delivery_payloads.agent_deliveries);
        self.reconcile_retired_agent_deliveries(tab_id, &owner);
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
        #[cfg(any(test, feature = "test-support"))]
        test_boundary::finished(tab_id);
    }

    fn reconcile_retired_agent_deliveries(
        &self,
        tab_id: &str,
        owner: &super::pending_delivery_payloads::agent_deliveries::AgentDeliveryStore,
    ) {
        let mut receipts = match owner.pending_receipts(tab_id) {
            Ok(receipts) => receipts,
            Err(error) => {
                log::error!(
                    "[agent-delivery] tab={} receipt read failed: {}",
                    tab_id,
                    error
                );
                return;
            }
        };
        if receipts.is_empty() {
            return;
        }
        let ledger = super::chat_send_transaction::ChatSendLedger::default();
        for receipt in &receipts {
            if receipt.status.is_none() {
                if let Some(anchor) = receipt.anchor.clone() {
                    ledger.track_retired_agent_delivery(
                        tab_id,
                        &receipt.id,
                        receipt.sequence,
                        &receipt.exact_text,
                        anchor,
                    );
                }
            }
        }
        super::chat_send_evidence::scan_transcript_evidence(
            &self.agent_registry,
            &ledger,
            tab_id,
            |_| {},
        );
        for receipt in &mut receipts {
            let worker_target = {
                let mux = self.mux.read();
                mux.resolve_tab_id(tab_id)
                    .and_then(|resolved| mux.get_tab(&resolved))
                    .is_some_and(|tab| worker_brief_flagged(tab.role))
            };
            if worker_target {
                receipt.worker_brief = true;
                if receipt.payload_kind == "agent_message" {
                    receipt.payload_kind = "worker_brief".into();
                }
            }
            if receipt.status.is_none() {
                let evidence = ledger.status(&receipt.id).map(|status| {
                    status.state == super::chat_send_transaction::ChatSendState::Sent
                });
                receipt.status = Some(retired_delivery_status(evidence));
                if let Err(error) = owner.save_pending_receipt(receipt) {
                    log::error!(
                        "[agent-delivery] tab={} delivery={} receipt update failed: {}",
                        tab_id,
                        receipt.id,
                        error
                    );
                }
            }
            if !receipt.settlement_notified {
                self.publish_retired_delivery_status(receipt);
                if receipt.status == Some(AgentDeliveryReceiptStatus::Delivered) {
                    let payload = AgentDeliveryPayload {
                        trace_id: None,
                        transport: super::messenger::QueuedAgentPayload::Text {
                            text: receipt.exact_text.clone(),
                        },
                        observer: receipt.observer.clone(),
                        idempotency_key: None,
                        expected_pty_id: None,
                        expected_session_id: None,
                        expected_binding_nonce: None,
                    };
                    self.record_agent_delivery_written(
                        tab_id,
                        Some(&receipt.id),
                        &payload,
                        true,
                        "delivered_after_restart",
                    );
                } else {
                    self.publish_retired_delivery_incident(receipt);
                }
                receipt.settlement_notified = true;
            }
            if let Err(error) = owner.complete_receipt(receipt) {
                log::error!(
                    "[agent-delivery] tab={} delivery={} receipt settlement failed: {}",
                    tab_id,
                    receipt.id,
                    error
                );
            }
        }
    }

    fn publish_retired_delivery_status(&self, receipt: &AgentDeliveryReceipt) {
        let state = match receipt.status {
            Some(AgentDeliveryReceiptStatus::Delivered) => "delivered",
            Some(AgentDeliveryReceiptStatus::SubmitUnconfirmed) | None => "submit_unconfirmed",
        };
        self.notify_event(
            "agent-delivery-status",
            serde_json::json!({
                "tab_id": receipt.tab_id,
                "delivery_id": receipt.id,
                "payload_kind": receipt.payload_kind,
                "worker_brief_flagged": receipt.worker_brief,
                "report_flagged": receipt.report,
                "status": state,
            }),
        );
    }

    fn publish_retired_delivery_incident(&self, receipt: &AgentDeliveryReceipt) {
        let (plan_id, task_id, attempt_id) = match receipt.observer.as_ref() {
            Some(DeliveryWriteObserver::Report {
                plan_id,
                task_id,
                attempt_id,
                ..
            }) => (
                plan_id.as_deref(),
                task_id.as_deref(),
                attempt_id.as_deref(),
            ),
            _ => (None, None, None),
        };
        self.notify_event(
            "coordination-incident",
            serde_json::json!({
                "kind": "agent_delivery_submit_unconfirmed",
                "tab_id": receipt.tab_id,
                "delivery_id": receipt.id,
                "payload_kind": receipt.payload_kind,
                "worker_brief_flagged": receipt.worker_brief,
                "report_flagged": receipt.report,
                "plan_id": plan_id,
                "task_id": task_id,
                "attempt_id": attempt_id,
                "impact": format!(
                    "Claimed delivery {} to tab {} ({}) has no exact own-session turn proof after restart.",
                    receipt.id,
                    receipt.tab_id,
                    receipt.payload_kind
                ),
                "cleanup": "The claimed payload was retired after its durable receipt was written.",
                "fallback": "The delivery remains submit_unconfirmed, was not replayed, and may need explicit coordination.",
            }),
        );
    }

    fn retired_delivery_recovery(
        &self,
        tab_id: &str,
        payload: &AgentDeliveryPayload,
    ) -> AgentDeliveryRecovery {
        let captured = self.chat_send_anchor(tab_id);
        let anchor = captured.anchor.filter(|anchor| {
            super::codex_rpc::lifecycle::for_tab(tab_id).is_none()
                && payload
                    .expected_session_id
                    .as_deref()
                    .is_some_and(|expected| anchor.session_id == expected)
                && captured.live_binding.as_ref().is_some_and(|binding| {
                    binding.session_id.as_deref() == payload.expected_session_id.as_deref()
                        && binding.binding_nonce.as_deref()
                            == payload.expected_binding_nonce.as_deref()
                })
        });
        let worker_target = {
            let mux = self.mux.read();
            mux.resolve_tab_id(tab_id)
                .and_then(|resolved| mux.get_tab(&resolved))
                .is_some_and(|tab| worker_brief_flagged(tab.role))
        };
        let report = matches!(
            payload.observer.as_ref(),
            Some(DeliveryWriteObserver::Report { .. })
        );
        let (worker_brief, report) = retired_delivery_flags(worker_target, report);
        let payload_kind = if report {
            "report"
        } else if worker_brief {
            "worker_brief"
        } else {
            match payload.observer.as_ref() {
                Some(DeliveryWriteObserver::PlanRole { .. }) => "plan_role",
                Some(DeliveryWriteObserver::PlanReminder { .. }) => "reminder",
                _ => "agent_message",
            }
        };
        let exact_text = match &payload.transport {
            super::messenger::QueuedAgentPayload::Text { text } => text.clone(),
            super::messenger::QueuedAgentPayload::RoutedFrame {
                opening_tag,
                body,
                suffix,
            } => codeterm_wire::domios::DomiosFrame {
                opening_tag: opening_tag.clone(),
                body: body.clone(),
                suffix: suffix.clone(),
            }
            .to_message(),
        };
        AgentDeliveryRecovery {
            payload_kind: payload_kind.into(),
            worker_brief,
            report,
            observer: payload.observer.clone(),
            exact_text,
            anchor,
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
        let recovery = self.retired_delivery_recovery(tab_id, payload);
        let claim = match owner.claim_with_recovery(tab_id, &record.id, recovery) {
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
        self.notify_event(
            "agent-delivery-status",
            serde_json::json!({
                "tab_id": tab_id,
                "delivery_id": delivery_id,
                "status": "failed",
                "code": "launch_ended_before_write",
                "bytes_written": false,
            }),
        );
        if let Some(flow) = payload.trace_id.as_deref() {
            crate::flow_trace::record(
                flow,
                "delivery.failed",
                tab_id,
                "",
                "launch_ended_before_write",
                Some(delivery_id),
            );
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

    pub(crate) fn record_runtime_delivery_written(
        &self,
        tab_id: &str,
        send_id: &str,
        observer: DeliveryWriteObserver,
    ) {
        let payload = AgentDeliveryPayload {
            trace_id: None,
            transport: super::messenger::QueuedAgentPayload::Text {
                text: String::new(),
            },
            observer: Some(observer),
            idempotency_key: Some(send_id.into()),
            expected_pty_id: None,
            expected_session_id: None,
            expected_binding_nonce: None,
        };
        self.record_agent_delivery_written(
            tab_id,
            Some(send_id),
            &payload,
            true,
            "runtime-written",
        );
    }

    fn record_agent_delivery_written(
        &self,
        tab_id: &str,
        delivery_id: Option<&str>,
        payload: &AgentDeliveryPayload,
        queued: bool,
        outcome: &str,
    ) {
        if let Some(id) = payload
            .trace_id
            .as_deref()
            .or(payload.idempotency_key.as_deref())
            .or(delivery_id)
        {
            crate::flow_trace::record(id, "message.written", tab_id, "", outcome, delivery_id);
        }
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

fn retired_delivery_status(evidence: Option<bool>) -> AgentDeliveryReceiptStatus {
    if evidence == Some(true) {
        AgentDeliveryReceiptStatus::Delivered
    } else {
        AgentDeliveryReceiptStatus::SubmitUnconfirmed
    }
}

fn retired_delivery_flags(worker_target: bool, report: bool) -> (bool, bool) {
    (worker_target, report)
}

fn worker_brief_flagged(role: Option<crate::multiplexer::types::AgentRole>) -> bool {
    role == Some(crate::multiplexer::types::AgentRole::Worker)
}

#[cfg(test)]
#[path = "agent_delivery_outbox/tests.rs"]
mod tests;

#[cfg(any(test, feature = "test-support"))]
pub(crate) mod test_boundary;
