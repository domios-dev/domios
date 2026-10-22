use super::*;

impl Messenger {
    pub(crate) fn deliver_turn_resume(
        svc: &crate::service::ServiceContext,
        to_tab: &str,
        expected_pty_id: Option<&str>,
        expected_session_id: &str,
        expected_binding_nonce: &str,
        _sender: Option<&crate::multiplexer::types::TurnResumeSender>,
        body: &str,
    ) -> Result<Delivery, String> {
        let envelope = MessageEnvelope::codeterm_reminder("turn-resume");
        let should_prefix = should_prefix_pty(envelope.kind, true, &envelope.from_tab, to_tab);
        let routed_frame = Self::from(envelope.clone()).delivery_frame(should_prefix, body);
        let wrapped = routed_frame
            .as_ref()
            .map(FramedDelivery::to_message)
            .unwrap_or_else(|| body.to_string());
        let (body, _pad_file) =
            svc.substitute_paste_limited_body(to_tab, body.to_string(), wrapped.len())?;
        let routed_frame = routed_frame.map(|frame| frame.with_body(body.clone()));
        let text = routed_frame
            .as_ref()
            .map(FramedDelivery::to_message)
            .unwrap_or(body);
        let pending = svc.attach_system_send(to_tab, &text)?;
        svc.ui.emit(
            "chat-send-status",
            serde_json::to_value(&pending).unwrap_or_default(),
        );
        let result = (|| {
            let frame = routed_frame.ok_or("turn continuation requires a frame")?;
            if let Some(pty_id) = expected_pty_id {
                svc.send_to_authenticated_agent_session_pinned_routed(
                    to_tab,
                    pty_id,
                    expected_session_id,
                    expected_binding_nonce,
                    frame,
                )?;
            } else {
                super::super::agent_runtime::transport::send_pinned(
                    svc,
                    to_tab,
                    &AgentInput::Framed(frame),
                    &pending.send_id,
                    expected_session_id,
                    expected_binding_nonce,
                )
                .map_err(|error| error.ledger_message())?;
            }
            Ok(())
        })();
        if let Some(status) = svc
            .chat_send_ledger
            .finish(&pending.send_id, result.clone())
        {
            svc.ui.emit(
                "chat-send-status",
                serde_json::to_value(&status).unwrap_or_default(),
            );
        }
        result?;
        Ok(if expected_pty_id.is_some() {
            Delivery::Pty { text }
        } else {
            Delivery::ChatBackend {
                message_id: pending.send_id,
            }
        })
    }
}
