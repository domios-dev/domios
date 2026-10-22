use super::*;

pub use codeterm_types::agent::chat_types::SystemSend;

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
        status.system_send = Some(SystemSend::new(content, sent_at_ms));
        let status = status.clone();
        Some(Self::with_retention_window(&state, status))
    }
}

impl crate::service::ServiceContext {
    pub(crate) fn attach_system_send(
        &self,
        to_tab: &str,
        text: &str,
    ) -> Result<ChatSendStatus, String> {
        let evidence = self.chat_send_anchor(to_tab);
        let turn_text = self.chat_send_turn_evidence(to_tab, text);
        let pending = self.chat_send_ledger.try_start_scoped_with_normalization(
            to_tab,
            &turn_text,
            evidence.anchor,
            evidence.live_binding,
            evidence.turn_normalization,
        )?;
        let pending = self
            .chat_send_ledger
            .attach_system_send(
                &pending.send_id,
                text,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
            )
            .expect("a new continuation remains in the ledger");
        Ok(pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codeterm_wire::envelope::{FrameKind, MessageSegment, ENVELOPE_GRAMMAR_VERSION};

    fn attached(content: &str) -> SystemSend {
        let ledger = ChatSendLedger::default();
        let pending = ledger.try_start_scoped("tab", content, None, None).unwrap();
        let status = ledger
            .attach_system_send(&pending.send_id, content, 1000)
            .unwrap();
        let system_send = status.system_send.unwrap();
        assert_eq!(system_send.content, content);
        assert_eq!(system_send.segments_version, ENVELOPE_GRAMMAR_VERSION);
        system_send
    }

    #[test]
    fn runtime_envelope_with_trailing_newlines_is_a_frame_not_text() {
        let frame = crate::service::messenger::Messenger::message("Sender", "abc12345")
            .routed_frame("opaque body\n");
        let wire = format!("{}\n", frame.to_message());
        let system_send = attached(&wire);
        let [MessageSegment::Frame {
            kind, body, tip, ..
        }, MessageSegment::Text { text }] = system_send.segments.as_slice()
        else {
            panic!(
                "expected a frame then its newline: {:?}",
                system_send.segments
            );
        };
        assert_eq!(text, "\n");
        assert_eq!(*kind, FrameKind::Tab);
        assert_eq!(body, "opaque body");
        assert!(tip.is_some());
    }

    #[test]
    fn turn_resume_pad_pointer_and_plain_bodies_carry_segments() {
        let pointer = crate::service::messenger::Messenger::from(
            crate::service::messenger::MessageEnvelope::from_codeterm(),
        )
        .routed_frame("opaque pointer C:/opaque/inbox/body.md")
        .to_message();
        let framed = attached(&pointer);
        assert!(matches!(
            framed.segments.as_slice(),
            [MessageSegment::Frame { kind: FrameKind::System, body, .. }]
                if body == "opaque pointer C:/opaque/inbox/body.md"
        ));
        let plain = attached("opaque plain body");
        assert_eq!(
            plain.segments,
            [MessageSegment::Text {
                text: "opaque plain body".into()
            }]
        );
    }

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
