use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemSend {
    pub content: String,
    pub sent_at_ms: u64,
}

impl ChatSendLedger {
    pub(crate) fn attach_system_send(
        &self,
        send_id: &str,
        content: &str,
        sent_at_ms: u64,
    ) -> Option<ChatSendStatus> {
        let mut state = self.state.lock();
        state
            .token_by_id
            .insert(send_id.to_string(), send_id.to_string());
        let status = state.by_id.get_mut(send_id)?;
        status.system_send = Some(SystemSend {
            content: content.to_string(),
            sent_at_ms,
        });
        let status = status.clone();
        Some(Self::with_retention_window(&state, status))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuation_uncertainty_keeps_one_bubble() {
        let ledger = ChatSendLedger::default();
        let pending = ledger
            .try_start_scoped(
                "tab",
                "opaque",
                None,
                Some(SendLiveBinding {
                    session_id: Some("own".into()),
                    binding_nonce: Some("launch".into()),
                }),
            )
            .unwrap();
        let bubble = ledger
            .attach_system_send(&pending.send_id, "opaque", 1000)
            .unwrap();
        assert_eq!(
            bubble.correlation_id.as_deref(),
            Some(pending.send_id.as_str())
        );
        let uncertain = ledger
            .finish(&pending.send_id, Err("submit_unconfirmed: opaque".into()))
            .unwrap();
        assert_eq!(uncertain.state, ChatSendState::Unknown);
        assert_eq!(uncertain.system_send, bubble.system_send);
        assert_eq!(uncertain.send_id, bubble.send_id);
        assert!(ledger
            .confirm_delivered_user_turns_scoped("tab", "sibling", Some("launch"), "opaque")
            .is_empty());
        let confirmed =
            ledger.confirm_delivered_user_turns_scoped("tab", "own", Some("launch"), "opaque");
        assert_eq!(confirmed.len(), 1);
        assert_eq!(confirmed[0].state, ChatSendState::Sent);
        assert_eq!(confirmed[0].system_send, bubble.system_send);
        assert_eq!(
            ledger
                .finish(&pending.send_id, Err("submit_unconfirmed: late".into()))
                .unwrap()
                .state,
            ChatSendState::Sent
        );
    }
}
