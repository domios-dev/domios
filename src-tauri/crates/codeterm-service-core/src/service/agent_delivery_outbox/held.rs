use super::*;
use crate::service::chat_send_transaction::{ChatSendState, ChatSendStatus};
use crate::service::pending_delivery_payloads::agent_deliveries::parse_agent_delivery_send_id;

impl ServiceContext {
    pub(super) fn refuse_expired_held_agent_delivery(
        &self,
        tab: &str,
        record: &AgentDeliveryRecord,
        generation: u64,
    ) -> AgentWriteResult {
        let owner = &self.pending_delivery_payloads.agent_deliveries;
        let result = owner
            .refuse_held(tab, &record.id, "initial_readiness_deadline_elapsed")
            .and_then(|_| self.settle_held_agent_delivery(tab, &record.id));
        match result {
            Ok(()) => AgentWriteResult::Continue,
            Err(error) => {
                log::error!("[agent-delivery] held settlement failed tab={tab}: {error}");
                owner.block_after_refusal(tab, generation);
                AgentWriteResult::Refused
            }
        }
    }

    pub fn cancel_retained_agent_send(&self, send_id: &str) -> Result<ChatSendStatus, String> {
        let (tab, id) = parse_agent_delivery_send_id(send_id)
            .ok_or("send-cancel requires an agent-pty retained send id")?;
        let owner = &self.pending_delivery_payloads.agent_deliveries;
        let status = owner.inspect(send_id)?.ok_or("retained send not found")?;
        if status.state == ChatSendState::Failed
            && status.error.as_deref() == Some("cancelled_before_write")
        {
            return Ok(status);
        }
        if status.state != ChatSendState::Pending {
            return Err("send cannot be cancelled: payload may already have been written; inspect send-status and never replay uncertain input".into());
        }
        if !owner.refuse_held(&tab, &id, "cancelled_before_write")? {
            return Err("send is no longer held; inspect send-status before sending again".into());
        }
        self.settle_held_agent_delivery(&tab, &id)?;
        self.wake_agent_deliveries_on_state_change(&tab);
        owner
            .inspect(send_id)?
            .ok_or_else(|| "cancelled send receipt unavailable".into())
    }

    pub(in crate::service) fn settle_held_agent_delivery(
        &self,
        tab: &str,
        id: &str,
    ) -> Result<(), String> {
        let owner = &self.pending_delivery_payloads.agent_deliveries;
        let Some(record) = owner
            .records(tab)?
            .into_iter()
            .find(|record| record.id == id)
        else {
            return Ok(());
        };
        let reason = record
            .terminal_failure
            .as_deref()
            .ok_or("held refusal has no reason")?;
        if record.durable_delivery.is_some() {
            let payload =
                serde_json::from_str::<AgentDeliveryPayload>(&owner.payload(tab, &record)?)
                    .map_err(|error| error.to_string())?;
            self.fail_durable_delivery(tab, owner, &record, &payload, reason)?;
            return Ok(());
        }
        if !owner.discard_expired(tab, id)? {
            return Ok(());
        }
        self.cancel_durable_expiry(tab, id);
        self.notify_event(
            "agent-delivery-status",
            serde_json::json!({
                "tab_id": tab, "delivery_id": id, "status": "failed", "code": reason,
                "bytes_written": false,
            }),
        );
        Ok(())
    }
}
