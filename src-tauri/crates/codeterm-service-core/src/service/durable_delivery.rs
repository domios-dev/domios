//! Explicit custody for never-written results addressed to an exact saved session.

use super::messenger::AgentDeliveryPayload;
use super::pending_delivery_payloads::agent_deliveries::{AgentDeliveryRecord, AgentDeliveryStore};
use super::ServiceContext;
use crate::cron::clock::{system_clock, SharedClock};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

mod binding_failure;
mod custody;
mod settlement;
mod wall_deadline;

pub const DELIVERY_TTL_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DurableDelivery {
    pub producer: String,
    pub producer_tab: String,
    pub created_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) saved_target: Option<SavedTarget>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct SavedTarget {
    pub(super) tab: String,
    pub(super) provider: String,
    pub(super) session: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct SavedLaunch {
    pub pty: String,
    pub session: String,
    pub nonce: String,
}

pub(crate) struct DurableDeliveryRuntime {
    clock: SharedClock,
    timers: Mutex<HashMap<(String, String), Option<wall_deadline::WallDeadline>>>,
    native_deadlines: bool,
    binding_failures: Mutex<binding_failure::BindingFailures>,
}

impl Default for DurableDeliveryRuntime {
    fn default() -> Self {
        Self {
            clock: system_clock(),
            timers: Mutex::new(HashMap::new()),
            native_deadlines: true,
            binding_failures: Mutex::new(binding_failure::BindingFailures::default()),
        }
    }
}

impl DurableDeliveryRuntime {
    #[cfg(test)]
    pub(crate) fn with_clock(clock: SharedClock) -> Self {
        Self {
            clock,
            timers: Mutex::new(HashMap::new()),
            native_deadlines: false,
            binding_failures: Mutex::new(binding_failure::BindingFailures::default()),
        }
    }
}

pub(super) enum PreparedDelivery {
    Ready,
    Held,
    Failed(&'static str),
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum SettlementOutcome {
    Settled,
    Held,
}

impl ServiceContext {
    fn durable_runtime(&self) -> &DurableDeliveryRuntime {
        self.extensions.get_or_init(DurableDeliveryRuntime::default)
    }

    pub fn durable_delivery(
        &self,
        producer: impl Into<String>,
        producer_tab: impl Into<String>,
    ) -> DurableDelivery {
        DurableDelivery {
            producer: producer.into(),
            producer_tab: producer_tab.into(),
            created_at_ms: self.durable_runtime().clock.now_ms(),
            saved_target: None,
        }
    }

    pub(super) fn durable_delivery_expired(&self, payload: &AgentDeliveryPayload) -> bool {
        payload.durable_delivery.as_ref().is_some_and(|delivery| {
            self.durable_runtime().clock.now_ms()
                >= delivery.created_at_ms.saturating_add(DELIVERY_TTL_MS)
        })
    }

    pub(crate) fn saved_delivery_target_matches(
        &self,
        target: &str,
        payload: &AgentDeliveryPayload,
    ) -> bool {
        let Some(saved) = payload
            .durable_delivery
            .as_ref()
            .and_then(|delivery| delivery.saved_target.as_ref())
        else {
            return true;
        };
        self.mux.read().get_tab(target).is_some_and(|tab| {
            saved.tab == target
                && tab.agent_type.as_deref() == Some(saved.provider.as_str())
                && tab.agent_session_id.as_deref() == Some(saved.session.as_str())
        })
    }

    pub(super) fn capture_saved_delivery(
        &self,
        target: &str,
        payload: &mut AgentDeliveryPayload,
    ) -> Result<bool, String> {
        let Some(delivery) = payload.durable_delivery.as_mut() else {
            return Ok(false);
        };
        let mux = self.mux.read();
        let tab = mux
            .get_tab(target)
            .ok_or("durable delivery recipient is missing")?;
        if !tab.restored_from_snapshot {
            return Ok(false);
        }
        if payload.expected_pty_id.is_some()
            || payload.expected_binding_nonce.is_some()
            || payload.expected_session_id.is_some()
        {
            return Err("durable delivery cannot adopt an existing launch authority".into());
        }
        let saved = SavedTarget {
            tab: target.into(),
            provider: tab
                .agent_type
                .clone()
                .ok_or("durable delivery has no saved provider")?,
            session: tab
                .agent_session_id
                .clone()
                .ok_or("durable delivery has no exact saved session")?,
        };
        drop(mux);
        self.resolve_pending_binding_failure(target)?;
        let body = format!(
            "Original run: {}; timestamp: {}\n{}",
            delivery.producer,
            delivery.created_at_ms,
            payload.transport.body()
        );
        let framed_bytes = payload
            .transport
            .clone()
            .with_body(body.clone())
            .to_message()
            .len();
        let (body, _) = self.substitute_paste_limited_body(target, body, framed_bytes)?;
        let transport = payload.transport.clone().with_body(body);
        if self
            .agent_registry
            .with_provider(&saved.provider, |provider| provider.paste_limit_bytes())
            .flatten()
            .is_some_and(|limit| transport.to_message().len() > limit)
        {
            return Err("held delivery pointer exceeds the provider paste limit".into());
        }
        payload.transport = transport;
        delivery.saved_target = Some(saved);
        Ok(true)
    }

    pub(super) fn prepare_saved_delivery(
        &self,
        target: &str,
        record: &AgentDeliveryRecord,
        payload: &mut AgentDeliveryPayload,
    ) -> Result<PreparedDelivery, String> {
        let Some(delivery) = payload.durable_delivery.as_ref() else {
            return Ok(PreparedDelivery::Ready);
        };
        if record.terminal_failure.is_some() {
            return Ok(PreparedDelivery::Failed("previous_terminal_failure"));
        }
        if self.durable_runtime().clock.now_ms()
            >= delivery.created_at_ms.saturating_add(DELIVERY_TTL_MS)
        {
            return Ok(PreparedDelivery::Failed("delivery_ttl_expired"));
        }
        let Some(saved) = &delivery.saved_target else {
            return Ok(PreparedDelivery::Ready);
        };
        let _claim = self.pty_ownership.acquire(target);
        self.resolve_pending_binding_failure(target)?;
        if self.binding_failure(target, record).is_some() {
            return Ok(PreparedDelivery::Failed("launch_authority_unavailable"));
        }
        {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(target) else {
                return Ok(PreparedDelivery::Failed("recipient_gone"));
            };
            if tab.agent_type.is_none() {
                return Ok(PreparedDelivery::Failed("recipient_agent_exited"));
            }
            if saved.tab != target || tab.agent_type.as_deref() != Some(&saved.provider) {
                return Ok(PreparedDelivery::Failed("saved_provider_changed"));
            }
            if tab
                .agent_session_id
                .as_deref()
                .is_some_and(|session| session != saved.session)
            {
                return Ok(PreparedDelivery::Failed("saved_session_changed"));
            }
            if tab.boot_stage == Some(codeterm_types::pty::BootStage::SpawnFailed) {
                return Ok(PreparedDelivery::Failed("recipient_launch_failed"));
            }
            if record.saved_launch.is_some() && tab.restored_from_snapshot {
                return Ok(PreparedDelivery::Failed("launch_ended_before_write"));
            }
            if tab.restored_from_snapshot
                || tab.agent_booting
                || !tab.launch_session_authenticated()
            {
                return Ok(PreparedDelivery::Held);
            }
        }
        let Some(bound) = &record.saved_launch else {
            return Ok(PreparedDelivery::Held);
        };
        payload.expected_pty_id = Some(bound.pty.clone());
        payload.expected_session_id = Some(bound.session.clone());
        payload.expected_binding_nonce = Some(bound.nonce.clone());
        Ok(PreparedDelivery::Ready)
    }

    pub(super) fn fail_durable_delivery(
        &self,
        target: &str,
        owner: &AgentDeliveryStore,
        record: &AgentDeliveryRecord,
        payload: &AgentDeliveryPayload,
        reason: &str,
    ) -> Result<SettlementOutcome, String> {
        let delivery = payload
            .durable_delivery
            .as_ref()
            .ok_or("missing durable delivery authority")?;
        let reason = record.terminal_failure.as_deref().unwrap_or(reason);
        let detail = record.terminal_detail.clone().or_else(|| {
            if reason == "launch_authority_unavailable" {
                return self.binding_failure(target, record);
            }
            (reason == "recipient_launch_failed")
                .then(|| {
                    self.mux
                        .read()
                        .get_tab(target)
                        .and_then(|tab| tab.boot_failure_reason.clone())
                })
                .flatten()
        });
        let (reason, detail) =
            owner.mark_terminal_failure(target, &record.id, reason, detail.as_deref())?;
        let failure = serde_json::json!({
            "tab_id": target, "delivery_id": record.id, "producer": delivery.producer,
            "producer_tab": delivery.producer_tab, "created_at_ms": delivery.created_at_ms,
            "status": "failed", "code": reason, "detail": detail, "bytes_written": false,
        });
        if self.emit_durable_failure(owner, target, &record.id, delivery, &reason, failure)?
            == SettlementOutcome::Held
        {
            return Ok(SettlementOutcome::Held);
        }
        let discarded = owner.discard_expired(target, &record.id)?;
        self.cancel_durable_expiry(target, &record.id);
        self.durable_runtime()
            .binding_failures
            .lock()
            .records
            .remove(&(target.into(), record.id.clone()));
        if discarded {
            owner.state_changed(target);
            if owner.has_pending(target)? {
                self.schedule_agent_delivery_flush(target);
            }
        }
        Ok(SettlementOutcome::Settled)
    }

    pub(super) fn schedule_durable_expiry(
        &self,
        target: &str,
        id: &str,
        payload: &AgentDeliveryPayload,
    ) {
        let Some(delivery) = &payload.durable_delivery else {
            return;
        };
        self.schedule_durable_metadata_expiry(target, id, delivery);
    }

    fn schedule_durable_metadata_expiry(&self, target: &str, id: &str, delivery: &DurableDelivery) {
        let runtime = self.durable_runtime();
        let mut timers = runtime.timers.lock();
        let key = (target.to_string(), id.to_string());
        if timers.contains_key(&key) {
            return;
        }
        let deadline = delivery.created_at_ms.saturating_add(DELIVERY_TTL_MS);
        let service = self.weak_self();
        let callback_key = key.clone();
        let timer = if runtime.native_deadlines {
            match wall_deadline::WallDeadline::arm(deadline, move || {
                if let Some(service) = service.upgrade() {
                    service
                        .durable_runtime()
                        .timers
                        .lock()
                        .remove(&callback_key);
                    service.wake_agent_deliveries_on_state_change(&callback_key.0);
                }
            }) {
                Ok(timer) => Some(timer),
                Err(error) => {
                    log::error!("[agent-delivery] wall deadline unavailable tab={target}: {error}");
                    let owner = &self.pending_delivery_payloads.agent_deliveries;
                    let marked = owner.mark_terminal_failure(
                        target,
                        id,
                        "delivery_deadline_unavailable",
                        Some(&error),
                    );
                    drop(timers);
                    match marked {
                        Ok(_) => self.wake_agent_deliveries_on_state_change(target),
                        Err(error) => log::error!("[agent-delivery] deadline refusal persistence failed tab={target}: {error}"),
                    }
                    return;
                }
            }
        } else {
            None
        };
        timers.insert(key, timer);
    }

    pub(super) fn cancel_durable_expiry(&self, target: &str, id: &str) {
        self.durable_runtime()
            .timers
            .lock()
            .remove(&(target.into(), id.into()));
    }

    pub(super) fn restore_durable_expiries(&self, target: &str) -> Result<(), String> {
        let owner = &self.pending_delivery_payloads.agent_deliveries;
        for record in owner.records(target)? {
            if let Some(delivery) = &record.durable_delivery {
                self.schedule_durable_metadata_expiry(target, &record.id, delivery);
            }
        }
        Ok(())
    }

    pub(super) fn expire_due_durable_deliveries(
        &self,
        target: &str,
        owner: &AgentDeliveryStore,
    ) -> Result<(), String> {
        for record in owner.records(target)? {
            if record.state
                != super::pending_delivery_payloads::agent_deliveries::AgentDeliveryState::Pending
            {
                continue;
            }
            let Some(delivery) = &record.durable_delivery else {
                continue;
            };
            if record.terminal_failure.is_some()
                || self.mux.read().get_tab(target).is_none()
                || self.durable_runtime().clock.now_ms()
                    >= delivery.created_at_ms.saturating_add(DELIVERY_TTL_MS)
            {
                let reason = if self.mux.read().get_tab(target).is_none() {
                    "recipient_gone"
                } else {
                    "delivery_ttl_expired"
                };
                let result = owner
                    .payload(target, &record)
                    .and_then(|text| {
                        serde_json::from_str::<AgentDeliveryPayload>(&text)
                            .map_err(|error| error.to_string())
                    })
                    .and_then(|payload| {
                        self.fail_durable_delivery(target, owner, &record, &payload, reason)
                    });
                if let Err(error) = result {
                    log::error!("[agent-delivery] durable settlement failed tab={target} record={}: {error}", record.id);
                }
            } else {
                self.schedule_durable_metadata_expiry(target, &record.id, delivery);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
