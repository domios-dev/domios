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
        let frame = routed_frame.ok_or("turn continuation requires a frame")?;
        let Some(pty_id) = expected_pty_id else {
            // The durable owner holds this behind a running turn and writes it once; the claim
            // already makes this the only delivery, so each one gets its own token.
            let authority = svc.user_send_authority(to_tab)?;
            if !super::super::agent_runtime::user_delivery::is_current_session(
                svc,
                &authority,
                expected_session_id,
                expected_binding_nonce,
            ) {
                return Err(crate::service::turn_resume::SESSION_CHANGED.into());
            }
            let token = format!("turn-resume:{}", uuid::Uuid::new_v4().simple());
            return super::super::agent_runtime::transport::send_envelope(
                svc, to_tab, &frame, &token, None,
            );
        };
        let pending = svc.attach_system_send(to_tab, &text)?;
        svc.ui.emit(
            "chat-send-status",
            serde_json::to_value(&pending).unwrap_or_default(),
        );
        let result = svc.send_to_authenticated_agent_session_pinned_routed(
            to_tab,
            pty_id,
            expected_session_id,
            expected_binding_nonce,
            frame,
        );
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
        Ok(Delivery::Pty { text })
    }
}
