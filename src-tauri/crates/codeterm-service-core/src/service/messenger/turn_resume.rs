use super::*;

impl Messenger {
    pub(crate) fn deliver_turn_resume(
        svc: &crate::service::ServiceContext,
        to_tab: &str,
        expected_pty_id: &str,
        expected_session_id: &str,
        expected_binding_nonce: &str,
        sender: Option<&crate::multiplexer::types::TurnResumeSender>,
        body: &str,
    ) -> Result<Delivery, String> {
        let envelope = sender
            .map(|sender| {
                let mut envelope =
                    MessageEnvelope::tab(sender.label.clone(), sender.tab_id.clone());
                envelope.from_agent = sender.from_agent;
                envelope.from_model = sender.from_model.clone();
                envelope
            })
            .unwrap_or_else(MessageEnvelope::from_codeterm);
        let should_prefix = should_prefix_pty(envelope.kind, true, &envelope.from_tab, to_tab);
        let routed_frame = should_prefix.then(|| Self::from(envelope.clone()).routed_frame(body));
        let wrapped = routed_frame
            .as_ref()
            .map(codeterm_wire::domios::DomiosFrame::to_message)
            .unwrap_or_else(|| body.to_string());
        let (body, _pad_file) =
            svc.substitute_paste_limited_body(to_tab, body.to_string(), wrapped.len())?;
        let routed_frame = routed_frame.map(|mut frame| {
            frame.body = body.clone();
            frame
        });
        let text = routed_frame
            .as_ref()
            .map(codeterm_wire::domios::DomiosFrame::to_message)
            .unwrap_or(body);
        let pending = svc.attach_system_send(to_tab, &text)?;
        codeterm_events::emit_everywhere(&svc.app_handle, "chat-send-status", &pending);
        let result = (|| {
            if let Some(frame) = routed_frame {
                svc.send_to_authenticated_agent_session_pinned_routed(
                    to_tab,
                    expected_pty_id,
                    expected_session_id,
                    expected_binding_nonce,
                    frame,
                )?;
            } else {
                svc.send_to_authenticated_agent_session_pinned(
                    to_tab,
                    expected_pty_id,
                    expected_session_id,
                    expected_binding_nonce,
                    &text,
                )?;
            }
            Ok(())
        })();
        if let Some(status) = svc
            .chat_send_ledger
            .finish(&pending.send_id, result.clone())
        {
            codeterm_events::emit_everywhere(&svc.app_handle, "chat-send-status", &status);
        }
        result?;
        Ok(Delivery::Pty { text })
    }
}
