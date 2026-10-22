//! Explicit custody for never-written results addressed to an exact saved session.

use super::messenger::AgentDeliveryPayload;
use super::pending_delivery_payloads::agent_deliveries::{AgentDeliveryRecord, AgentDeliveryStore};
use super::ServiceContext;
use crate::cron::clock::{system_clock, SharedClock};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    tab: String,
    provider: String,
    session: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct SavedLaunch {
    pub pty: String,
    pub session: String,
    pub nonce: String,
}

pub(crate) struct DurableDeliveryRuntime {
    clock: SharedClock,
    timers: Mutex<HashMap<(String, String), tokio::task::AbortHandle>>,
}

impl Default for DurableDeliveryRuntime {
    fn default() -> Self {
        Self::with_clock(system_clock())
    }
}

impl DurableDeliveryRuntime {
    pub(crate) fn with_clock(clock: SharedClock) -> Self {
        Self {
            clock,
            timers: Mutex::new(HashMap::new()),
        }
    }
}

impl Drop for DurableDeliveryRuntime {
    fn drop(&mut self) {
        for (_, timer) in self.timers.get_mut().drain() {
            timer.abort();
        }
    }
}

pub(super) enum PreparedDelivery {
    Ready,
    Held,
    Failed(&'static str),
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
        delivery.saved_target = Some(SavedTarget {
            tab: target.into(),
            provider: tab
                .agent_type
                .clone()
                .ok_or("durable delivery has no saved provider")?,
            session: tab
                .agent_session_id
                .clone()
                .ok_or("durable delivery has no exact saved session")?,
        });
        Ok(true)
    }

    pub(super) fn prepare_saved_delivery(
        &self,
        target: &str,
        owner: &AgentDeliveryStore,
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
        let observed = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(target) else {
                return Ok(PreparedDelivery::Failed("recipient_gone"));
            };
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
            let Some(pty) = tab.pty_id.clone() else {
                return Ok(PreparedDelivery::Held);
            };
            SavedLaunch {
                pty,
                session: tab
                    .agent_session_id
                    .clone()
                    .ok_or("authenticated recipient has no session")?,
                nonce: tab
                    .session_binding_nonce
                    .clone()
                    .ok_or("authenticated recipient has no nonce")?,
            }
        };
        let bound = owner.bind_saved_launch(target, &record.id, observed)?;
        payload.expected_pty_id = Some(bound.pty);
        payload.expected_session_id = Some(bound.session);
        payload.expected_binding_nonce = Some(bound.nonce);
        Ok(PreparedDelivery::Ready)
    }

    pub(super) fn fail_durable_delivery(
        &self,
        target: &str,
        owner: &AgentDeliveryStore,
        record: &AgentDeliveryRecord,
        payload: &AgentDeliveryPayload,
        reason: &str,
    ) -> Result<(), String> {
        let delivery = payload
            .durable_delivery
            .as_ref()
            .ok_or("missing durable delivery authority")?;
        let reason = record.terminal_failure.as_deref().unwrap_or(reason);
        owner.mark_terminal_failure(target, &record.id, reason)?;
        let failure = serde_json::json!({
            "tab_id": target, "delivery_id": record.id, "producer": delivery.producer,
            "producer_tab": delivery.producer_tab, "created_at_ms": delivery.created_at_ms,
            "status": "failed", "code": reason, "bytes_written": false,
        });
        let receipt = self
            .ensure_agent_pad(&delivery.producer_tab)?
            .write_inbox_body(
                serde_json::to_string_pretty(&failure)
                    .map_err(|error| error.to_string())?
                    .as_bytes(),
            )?;
        let producer_live = self
            .mux
            .read()
            .get_tab(&delivery.producer_tab)
            .is_some_and(|tab| tab.agent_type.is_some());
        let notice = if producer_live {
            self.create_agent_notification_reported(
            "Durable result delivery failed".into(),
            Some(format!("Producer: {}\nOriginal timestamp: {}\nRecipient: {target}\nReason: {reason}\nDelivery: {}\nThe result was not written and will not be replayed.", delivery.producer, delivery.created_at_ms, record.id)),
            Some("error"), None, Some(true),
            super::notification::NotificationOrigin::local_tab(delivery.producer_tab.clone()),
            super::notification::PushIntent {
                level: Some(codeterm_notification_core::contract::NotifyLevel::Failure),
                event_key: Some(format!("durable-delivery-failed:{}", record.id)),
            }, None,
        ).map(|_| ())
        } else {
            Ok(())
        };
        if let Err(error) = notice {
            log::error!(
                "[agent-delivery] producer failure notice unavailable: {error}; receipt={}",
                receipt.display()
            );
        }
        self.notify_event("agent-delivery-status", failure);
        owner.discard_expired(target, &record.id)?;
        self.cancel_durable_expiry(target, &record.id);
        Ok(())
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
        let runtime = self.durable_runtime();
        let mut timers = runtime.timers.lock();
        let key = (target.to_string(), id.to_string());
        if timers.contains_key(&key) {
            return;
        }
        let remaining = delivery
            .created_at_ms
            .saturating_add(DELIVERY_TTL_MS)
            .saturating_sub(runtime.clock.now_ms());
        let service = self.weak_self();
        let tab = target.to_string();
        let task = crate::async_runtime::spawn(async move {
            let _ = tokio::time::timeout(
                std::time::Duration::from_millis(remaining),
                std::future::pending::<()>(),
            )
            .await;
            if let Some(service) = service.upgrade() {
                service.wake_agent_deliveries_on_state_change(&tab);
            }
        });
        timers.insert(key, task.abort_handle());
    }

    pub(super) fn cancel_durable_expiry(&self, target: &str, id: &str) {
        if let Some(timer) = self
            .durable_runtime()
            .timers
            .lock()
            .remove(&(target.into(), id.into()))
        {
            timer.abort();
        }
    }

    pub(super) fn restore_durable_expiries(&self, target: &str) -> Result<(), String> {
        let owner = &self.pending_delivery_payloads.agent_deliveries;
        for record in owner.records(target)? {
            let payload = serde_json::from_str(&owner.payload(target, &record)?)
                .map_err(|_| "durable delivery payload decode failed")?;
            self.schedule_durable_expiry(target, &record.id, &payload);
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
            let payload: AgentDeliveryPayload =
                serde_json::from_str(&owner.payload(target, &record)?)
                    .map_err(|_| "durable delivery payload decode failed")?;
            if let Some(delivery) = &payload.durable_delivery {
                if record.terminal_failure.is_some()
                    || self.durable_runtime().clock.now_ms()
                        >= delivery.created_at_ms.saturating_add(DELIVERY_TTL_MS)
                {
                    self.fail_durable_delivery(
                        target,
                        owner,
                        &record,
                        &payload,
                        "delivery_ttl_expired",
                    )?;
                }
            }
        }
        Ok(())
    }
}
