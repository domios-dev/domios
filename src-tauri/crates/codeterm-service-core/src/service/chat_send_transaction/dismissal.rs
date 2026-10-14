use super::*;
use crate::service::pending_delivery_payloads::user_sends::dismissal::UserSendDismissal;

impl crate::service::ServiceContext {
    pub(crate) fn delete_chat_send(
        &self,
        tab_id: &str,
        mut dismissal: UserSendDismissal,
    ) -> Result<UserSendDismissal, String> {
        if let Some(receipt) = self
            .pending_delivery_payloads
            .user_sends
            .dismissals(tab_id)?
            .into_iter()
            .find(|receipt| {
                receipt.send_id == dismissal.send_id
                    || receipt.send.correlation_id == dismissal.send.correlation_id
            })
        {
            return Ok(receipt);
        }
        self.restore_durable_user_statuses(tab_id)?;
        self.reconcile_chat_send_evidence(tab_id);
        let status = self
            .chat_send_ledger
            .status(&dismissal.send_id)
            .or_else(|| {
                self.chat_send_ledger
                    .reconcile(tab_id)
                    .into_iter()
                    .find(|status| {
                        status.correlation_id.as_deref()
                            == Some(dismissal.send.correlation_id.as_str())
                    })
            });
        if status
            .as_ref()
            .is_some_and(|status| !deletion_allowed(status, tab_id, &dismissal.send.correlation_id))
        {
            return Err("chat send cannot be deleted".into());
        }
        if let Some(status) = status {
            dismissal.send_id = status.send_id;
        }
        let receipt = self
            .pending_delivery_payloads
            .user_sends
            .cancel_or_dismiss(tab_id, dismissal)?;
        self.wake_parked_user_send(tab_id);
        codeterm_events::emit_everywhere(
            &self.app_handle,
            "chat-send-dismissed",
            &serde_json::json!({"tab_id": tab_id, "receipt": receipt}),
        );
        Ok(receipt)
    }
}

fn deletion_allowed(status: &ChatSendStatus, tab_id: &str, token: &str) -> bool {
    status.tab_id == tab_id
        && status.correlation_id.as_deref() == Some(token)
        && status.system_send.is_none()
        && status.state != ChatSendState::Sent
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivered_or_foreign_send_is_refused() {
        let ledger = ChatSendLedger::default();
        let status = ledger.try_start("tab", "opaque", None).unwrap();
        let status = ledger
            .attach_client_token(&status.send_id, "token".into())
            .unwrap();
        assert!(deletion_allowed(&status, "tab", "token"));
        assert!(!deletion_allowed(&status, "other", "token"));
        assert!(!deletion_allowed(&status, "tab", "other"));
        assert!(!deletion_allowed(
            &ChatSendStatus {
                state: ChatSendState::Sent,
                ..status
            },
            "tab",
            "token"
        ));
    }
}
