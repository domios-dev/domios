use super::*;

struct NotificationClaim<'a> {
    owner: &'a AgentDeliveryStore,
    target: &'a str,
    id: &'a str,
}

impl Drop for NotificationClaim<'_> {
    fn drop(&mut self) {
        self.owner
            .release_terminal_notification(self.target, self.id);
    }
}

impl ServiceContext {
    pub(super) fn emit_durable_failure(
        &self,
        owner: &AgentDeliveryStore,
        target: &str,
        id: &str,
        delivery: &DurableDelivery,
        reason: &str,
        failure: serde_json::Value,
    ) -> Result<SettlementOutcome, String> {
        if !owner.claim_terminal_notification(target, id)? {
            return Ok(if owner.terminal_notification_emitted(target, id)? {
                SettlementOutcome::Settled
            } else {
                SettlementOutcome::Held
            });
        }
        let _claim = NotificationClaim { owner, target, id };
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let (receipt, _) = self.ensure_agent_pad(&delivery.producer_tab)?
                .write_delivery_receipt(id, serde_json::to_string_pretty(&failure)
                    .map_err(|error| error.to_string())?.as_bytes())?;
            let producer_live = self.mux.read().get_tab(&delivery.producer_tab)
                .is_some_and(|tab| tab.agent_type.is_some());
            if producer_live {
                if let Err(error) = self.create_agent_notification_reported(
                    "Durable result delivery failed".into(),
                    Some(format!("Producer: {}\nOriginal timestamp: {}\nRecipient: {target}\nReason: {reason}\nDelivery: {id}\nThe result was not written and will not be replayed.", delivery.producer, delivery.created_at_ms)),
                    Some("error"), None, Some(true),
                    super::super::notification::NotificationOrigin::local_tab(delivery.producer_tab.clone()),
                    super::super::notification::PushIntent {
                        level: Some(codeterm_notification_core::contract::NotifyLevel::Failure),
                        event_key: Some(format!("durable-delivery-failed:{id}")),
                    }, None,
                ) {
                    log::error!("[agent-delivery] producer failure notice unavailable: {error}; receipt={}", receipt.display());
                }
            }
            self.notify_event("agent-delivery-status", failure);
            owner.finish_terminal_notification(target, id)
        }))
        .map_err(|_| "durable failure notifier panicked; settlement retained for retry".to_string())??;
        Ok(SettlementOutcome::Settled)
    }
}
