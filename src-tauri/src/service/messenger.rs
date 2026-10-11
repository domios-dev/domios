//! Gateway for cross-tab / report / mesh deliveries. Routed PTY input carries a
//! typed XML tag around a bracketed body paste; chatBackend keeps structured
//! host-side transcript metadata.

mod turn_resume;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MessageKind {
    /// A direct message from one tab (or peer) to another.
    #[default]
    Message,
    /// A hierarchy / cron report bubbling up to a parent or inbox.
    Report,
    /// A host-owned event with no reply address.
    System,
    /// A fired delayed reminder injected back into a tab.
    Reminder,
    Mesh,
    Telegram,
    Pebble,
}

#[derive(Debug, Default, Clone)]
pub struct MessageEnvelope {
    pub kind: MessageKind,
    pub from_label: String,
    pub from_tab: String,
    pub from_agent: Option<bool>,
    pub relay_hops: u8,
    pub relay_notice: bool,
    pub from_model: Option<String>,
    pub purpose: Option<String>,
    pub from_mesh: Option<String>,
    pub from_plugin: Option<String>,
    pub plugin_icon_svg: Option<String>,
    pub peer: Option<String>,
    pub set_at: Option<u64>,
    pub fire_at: Option<u64>,
    pub id: Option<String>,
    pub(crate) trace_id: Option<String>,
    pub context: Option<String>,
    pub every_ms: Option<u64>,
    pub next_at_ms: Option<u64>,
    pub skipped_ticks: Option<u64>,
    pub(crate) delivery_observer: Option<DeliveryWriteObserver>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum DeliveryWriteObserver {
    Report {
        report_id: String,
        plan_id: Option<String>,
        task_id: Option<String>,
        attempt_id: Option<String>,
    },
    PlanRole {
        plan_id: String,
        role: String,
    },
    PlanReminder {
        plan_id: Option<String>,
        tab_id: String,
        reminder_kind: String,
        state_key: Option<String>,
        complete_wrapup_plan_id: Option<String>,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "transport", rename_all = "snake_case")]
pub(crate) enum QueuedAgentPayload {
    Text {
        text: String,
    },
    RoutedFrame {
        opening_tag: String,
        body: String,
        suffix: String,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct AgentDeliveryPayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) trace_id: Option<String>,
    pub(crate) transport: QueuedAgentPayload,
    pub(crate) observer: Option<DeliveryWriteObserver>,
    #[serde(default)]
    pub(crate) idempotency_key: Option<String>,
    #[serde(default)]
    pub(crate) expected_pty_id: Option<String>,
    #[serde(default)]
    pub(crate) expected_session_id: Option<String>,
    #[serde(default)]
    pub(crate) expected_binding_nonce: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct Messenger {
    pub envelope: MessageEnvelope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    Pty { text: String },
    ChatBackend { message_id: String },
    Pending { delivery_id: String },
}

impl Delivery {
    pub(crate) fn is_written(&self) -> bool {
        !matches!(self, Self::Pending { .. })
    }
}

impl MessageKind {
    fn marker_kind(self) -> &'static str {
        match self {
            Self::Message => "tab",
            Self::Report => "report",
            Self::System => "system",
            Self::Reminder => "reminder",
            Self::Mesh => "mesh",
            Self::Telegram => "telegram",
            Self::Pebble => "pebble",
        }
    }

    fn inbound_kind(self) -> &'static str {
        match self {
            Self::Message => "tab",
            Self::Report => "report",
            Self::System => "system",
            Self::Reminder => "reminder",
            Self::Mesh => "mesh",
            Self::Telegram => "telegram",
            Self::Pebble => "pebble",
        }
    }
}

impl MessageEnvelope {
    /// CodeTerm's own sender identity — single source so no path hardcodes it.
    pub const CODETERM: &'static str = "Domios";
    /// A host event from CodeTerm with no reply address.
    pub fn from_codeterm() -> Self {
        Self::system(None)
    }

    /// A host event from CodeTerm tagged with its internal context.
    pub fn codeterm_reminder(context: impl Into<String>) -> Self {
        Self::system(Some(context.into()))
    }

    fn system(context: Option<String>) -> Self {
        Self {
            kind: MessageKind::System,
            from_label: Self::CODETERM.to_string(),
            context: context.filter(|value| !value.trim().is_empty()),
            ..Default::default()
        }
    }

    pub fn tab(from_label: impl Into<String>, from_tab: impl Into<String>) -> Self {
        Self {
            kind: MessageKind::Message,
            from_label: from_label.into(),
            from_tab: from_tab.into(),
            ..Default::default()
        }
    }

    pub fn report(from_label: impl Into<String>, from_tab: impl Into<String>) -> Self {
        Self {
            kind: MessageKind::Report,
            from_label: from_label.into(),
            from_tab: from_tab.into(),
            ..Default::default()
        }
    }

    pub fn reminder(from_label: impl Into<String>, from_tab: impl Into<String>) -> Self {
        Self {
            kind: MessageKind::Reminder,
            from_label: from_label.into(),
            from_tab: from_tab.into(),
            ..Default::default()
        }
    }

    pub fn pebble() -> Self {
        Self {
            kind: MessageKind::Pebble,
            from_label: "Pebble".into(),
            ..Default::default()
        }
    }

    pub(crate) fn plugin(plugin_id: impl Into<String>, icon_svg: Option<String>) -> Self {
        Self {
            kind: MessageKind::Pebble,
            from_label: "Pebble".into(),
            from_plugin: Some(plugin_id.into()),
            plugin_icon_svg: icon_svg,
            ..Default::default()
        }
    }

    pub fn model(mut self, model: Option<String>) -> Self {
        self.from_model = model.filter(|m| !m.trim().is_empty());
        self
    }

    pub fn purpose(mut self, purpose: Option<String>) -> Self {
        self.purpose = purpose.filter(|p| !p.trim().is_empty());
        self
    }

    pub fn mesh(mut self, mesh: Option<String>) -> Self {
        self.from_mesh = mesh.filter(|m| !m.trim().is_empty());
        self
    }

    pub fn from_agent(mut self, from_agent: Option<bool>) -> Self {
        self.from_agent = from_agent;
        self
    }

    pub fn peer(mut self, peer: Option<String>) -> Self {
        self.peer = peer.filter(|p| !p.trim().is_empty());
        self
    }

    pub fn set_at(mut self, ms: u64) -> Self {
        self.set_at = Some(ms);
        self
    }

    pub fn fire_at(mut self, ms: u64) -> Self {
        self.fire_at = Some(ms);
        self
    }

    pub fn id(mut self, id: Option<String>) -> Self {
        self.id = id.filter(|s| !s.trim().is_empty());
        self
    }

    pub fn recurrence(
        mut self,
        every_ms: u64,
        next_at_ms: Option<u64>,
        skipped_ticks: u64,
    ) -> Self {
        self.every_ms = Some(every_ms);
        self.next_at_ms = next_at_ms;
        self.skipped_ticks = Some(skipped_ticks);
        self
    }
}

fn short_id(id: &str) -> &str {
    id.split('-').next().unwrap_or(id)
}

fn clean_name(name: &str) -> String {
    name.chars()
        .filter(|c| *c != '[' && *c != ']')
        .collect::<String>()
        .trim()
        .to_string()
}

fn safe_reply_token(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

impl From<MessageEnvelope> for Messenger {
    fn from(envelope: MessageEnvelope) -> Self {
        Self { envelope }
    }
}

impl Messenger {
    pub(crate) const AGENT_REPLY_HINT_PREFIX: &'static str = "[to reply, run: ";
    pub(crate) const AGENT_REPLY_COMMAND: &'static str = "codeterm send";
    const SHELL_REPLY_HINT: &'static str = "[sender is a shell; reply here]";
    pub fn message(from_name: impl Into<String>, from_tab: impl Into<String>) -> Self {
        MessageEnvelope::tab(from_name, from_tab).into()
    }

    pub fn report(from_name: impl Into<String>, from_tab: impl Into<String>) -> Self {
        MessageEnvelope::report(from_name, from_tab).into()
    }

    pub fn reminder(from_name: impl Into<String>, from_tab: impl Into<String>) -> Self {
        MessageEnvelope::reminder(from_name, from_tab).into()
    }

    pub fn set_at(mut self, ms: u64) -> Self {
        self.envelope.set_at = Some(ms);
        self
    }

    pub fn fire_at(mut self, ms: u64) -> Self {
        self.envelope.fire_at = Some(ms);
        self
    }

    pub fn id(mut self, id: Option<String>) -> Self {
        self.envelope.id = id.filter(|s| !s.trim().is_empty());
        self
    }

    pub fn model(mut self, model: Option<String>) -> Self {
        self.envelope.from_model = model.filter(|m| !m.trim().is_empty());
        self
    }

    pub fn purpose(mut self, purpose: Option<String>) -> Self {
        self.envelope.purpose = purpose.filter(|p| !p.trim().is_empty());
        self
    }

    pub fn mesh(mut self, mesh: Option<String>) -> Self {
        self.envelope.from_mesh = mesh.filter(|m| !m.trim().is_empty());
        self
    }

    pub fn from_agent(mut self, from_agent: Option<bool>) -> Self {
        self.envelope.from_agent = from_agent;
        self
    }

    pub fn peer(mut self, peer: Option<String>) -> Self {
        self.envelope.peer = peer.filter(|p| !p.trim().is_empty());
        self
    }

    pub fn pty_prefix(envelope: &MessageEnvelope) -> String {
        Self {
            envelope: envelope.clone(),
        }
        .build()
    }

    pub fn routed_frame(&self, body: &str) -> codeterm_types::domios::DomiosFrame {
        codeterm_types::domios::DomiosFrame {
            opening_tag: self.build(),
            body: body.to_string(),
            suffix: self
                .reply_hint()
                .map(|hint| format!(" {hint}"))
                .unwrap_or_default(),
        }
    }

    pub fn deliver(
        svc: &super::ServiceContext,
        to_tab: &str,
        envelope: MessageEnvelope,
        body: &str,
    ) -> Result<Delivery, String> {
        Self::deliver_with_pad_file(svc, to_tab, envelope, body).map(|(delivery, _)| delivery)
    }

    pub fn deliver_with_pad_file(
        svc: &super::ServiceContext,
        to_tab: &str,
        mut envelope: MessageEnvelope,
        body: &str,
    ) -> Result<(Delivery, Option<std::path::PathBuf>), String> {
        let kind = envelope.kind;
        if crate::paths::is_dev_instance() && envelope.id.is_none() {
            envelope.id = Some(uuid::Uuid::new_v4().to_string());
        }
        let trace_id = crate::paths::is_dev_instance()
            .then(|| envelope.id.clone())
            .flatten();
        let flow = crate::flow_trace::envelope_flow(svc, to_tab, &envelope);
        let provider = if trace_id.is_some() {
            svc.mux
                .read()
                .get_tab(to_tab)
                .and_then(|tab| tab.agent_type.clone())
                .unwrap_or_default()
        } else {
            String::new()
        };
        if let Some(id) = trace_id.as_deref() {
            crate::flow_trace::record_flow(
                id,
                "message.started",
                to_tab,
                &provider,
                "accepted",
                flow.as_deref(),
            );
        }
        let result = Self::deliver_inner(svc, to_tab, envelope, body);
        if let Some(id) = trace_id.as_deref() {
            let (outcome, related) = match &result {
                Ok((Delivery::Pending { delivery_id }, _)) => ("held", Some(delivery_id.as_str())),
                Ok(_) => ("written", None),
                Err(_) => ("refused", None),
            };
            crate::flow_trace::record_flow(
                id,
                "message.transport",
                to_tab,
                &provider,
                outcome,
                flow.as_deref(),
            );
            if let Some(related) = related {
                crate::flow_trace::record(
                    flow.as_deref().unwrap_or(id),
                    "delivery.queued",
                    to_tab,
                    &provider,
                    outcome,
                    Some(related),
                );
            }
        }
        if result
            .as_ref()
            .is_ok_and(|(delivery, _)| delivery.is_written())
        {
            svc.record_kpi_delivery(to_tab, kind, body.len());
        }
        result
    }

    fn deliver_inner(
        svc: &super::ServiceContext,
        to_tab: &str,
        envelope: MessageEnvelope,
        body: &str,
    ) -> Result<(Delivery, Option<std::path::PathBuf>), String> {
        if let Some(refusal) = svc.tab_surface_input_refusal(to_tab) {
            return Err(refusal);
        }
        if svc.tab_chat_backing(to_tab).is_some() {
            svc.chatbackend_reopen_if_restored(to_tab)?;
            let (plugin_id, session_id) = svc
                .tab_chat_backing(to_tab)
                .ok_or_else(|| "chatBackend Tab lost its backing".to_string())?;
            return dispatch_delivery(
                DeliveryTransport::ChatBackend,
                || {
                    Self::deliver_chatbackend(
                        svc,
                        to_tab,
                        &plugin_id,
                        &session_id,
                        envelope.clone(),
                        body,
                    )
                },
                || unreachable!(),
            )
            .map(|delivery| (delivery, None));
        }

        let (resolved_target, should_prefix, pty_agent_target) = {
            let mux = svc.mux.write();
            let resolved_target = mux
                .resolve_tab_id(to_tab)
                .unwrap_or_else(|| to_tab.to_string());
            let tab = mux.get_tab(&resolved_target);
            let target_is_agent =
                tab.is_some_and(|tab| tab.agent_type.is_some() || tab.agent_booting);
            (
                resolved_target.clone(),
                should_prefix_pty(
                    envelope.kind,
                    target_is_agent,
                    &envelope.from_tab,
                    &resolved_target,
                ),
                target_is_agent
                    && matches!(
                        tab.map(|p| &p.backing),
                        Some(crate::multiplexer::types::TabBacking::Pty)
                    ),
            )
        };
        log::debug!("[messenger] target={resolved_target} branch=pty agent={pty_agent_target} source_peer={:?}", envelope.peer);
        let entry = pty_submit_entry(envelope.kind);
        if envelope.kind == MessageKind::Message {
            super::provider_signals::warn_dispatch(svc, &envelope.from_tab, &resolved_target);
        }
        let messenger = Self::from(envelope.clone());
        let routed_frame = should_prefix.then(|| messenger.routed_frame(body));
        let text = if let Some(frame) = &routed_frame {
            frame.to_message()
        } else {
            body.to_string()
        };
        let (prepared_body, pad_file) =
            svc.substitute_paste_limited_body(&resolved_target, body.to_string(), text.len())?;
        let routed_frame = routed_frame.map(|mut frame| {
            frame.body = prepared_body.clone();
            frame
        });
        let text = routed_frame
            .as_ref()
            .map(codeterm_types::domios::DomiosFrame::to_message)
            .unwrap_or(prepared_body);
        let relay_id = svc.prepare_reply_relay(
            &resolved_target,
            &envelope,
            &text,
            should_prefix && messenger.reply_hint().is_some(),
        );
        let result: Result<(Delivery, Option<std::path::PathBuf>), String> = dispatch_delivery(
            DeliveryTransport::Pty,
            || unreachable!(),
            || {
                let delivery =
                    crate::service::pty_io::submit_transaction::submit_entry(entry, &text, |_| {
                        if pty_agent_target {
                            let transport = match routed_frame {
                                Some(frame) => QueuedAgentPayload::RoutedFrame {
                                    opening_tag: frame.opening_tag,
                                    body: frame.body,
                                    suffix: frame.suffix,
                                },
                                None => QueuedAgentPayload::Text { text: text.clone() },
                            };
                            svc.deliver_agent_payload(
                                &resolved_target,
                                AgentDeliveryPayload {
                                    trace_id: crate::flow_trace::envelope_flow(
                                        svc,
                                        &resolved_target,
                                        &envelope,
                                    ),
                                    transport,
                                    observer: envelope.delivery_observer.clone(),
                                    idempotency_key: envelope.id.clone(),
                                    expected_pty_id: None,
                                    expected_session_id: None,
                                    expected_binding_nonce: None,
                                },
                                &text,
                            )
                        } else {
                            if let Err(error) = svc.send_text(&resolved_target, &text) {
                                if !crate::service::delivery_error::DeliveryError::from_opaque(
                                    &resolved_target,
                                    error.clone(),
                                )
                                .bytes_written()
                                {
                                    return Err(error);
                                }
                                log::warn!(
                                    "[messenger] target={} outcome=written_submit_unconfirmed",
                                    resolved_target
                                );
                            }
                            Ok(Delivery::Pty { text: text.clone() })
                        }
                    })?;
                Ok((delivery, pad_file))
            },
        );
        if let (Some(id), Err(error)) = (relay_id, &result) {
            if !crate::service::delivery_error::DeliveryError::from_opaque(
                &resolved_target,
                error.clone(),
            )
            .bytes_written()
            {
                svc.reply_relays.cancel(&resolved_target, &id);
                if let Err(error) = svc.settle_restored_worker_turn(&resolved_target, &id) {
                    log::error!("[tool-less-restart] cancellation persistence failed: {error}");
                }
            }
        }
        result
    }

    pub fn deliver_planner_session(
        svc: &super::ServiceContext,
        to_tab: &str,
        expected_session_id: &str,
        expected_binding_nonce: &str,
        envelope: MessageEnvelope,
        body: &str,
        message_id: String,
    ) -> Result<Delivery, String> {
        let is_chatbackend = {
            let mux = svc.mux.read();
            let resolved = mux
                .resolve_tab_id(to_tab)
                .ok_or_else(|| "agent_unavailable: planner Tab missing".to_string())?;
            matches!(
                mux.get_tab(&resolved).map(|tab| &tab.backing),
                Some(crate::multiplexer::types::TabBacking::ChatBackend { .. })
            )
        };
        if is_chatbackend {
            svc.chatbackend_reopen_if_restored(to_tab)?;
        }
        let backing = {
            let mux = svc.mux.read();
            let resolved = mux
                .resolve_tab_id(to_tab)
                .ok_or_else(|| "agent_unavailable: planner Tab missing".to_string())?;
            let tab = mux
                .get_tab(&resolved)
                .ok_or_else(|| "agent_unavailable: planner Tab missing".to_string())?;
            if tab.agent_session_id.as_deref() != Some(expected_session_id)
                || tab.session_binding_nonce.as_deref() != Some(expected_binding_nonce)
                || tab.reported_session_binding_nonce.as_deref() != Some(expected_binding_nonce)
            {
                return Err("agent_unavailable: planner session is stale".into());
            }
            tab.backing.clone()
        };
        if matches!(backing, crate::multiplexer::types::TabBacking::Pty) {
            let observer = envelope.delivery_observer.clone();
            let idempotency_key = envelope.id.clone().or(Some(message_id.clone()));
            let messenger = Self::from(envelope);
            let mut frame = messenger.routed_frame(body);
            let wrapped = frame.to_message();
            let (body, _pad_file) =
                svc.substitute_paste_limited_body(to_tab, body.to_string(), wrapped.len())?;
            frame.body = body;
            let text = frame.to_message();
            let pty_id = svc.resolve_pty_id(to_tab)?;
            let payload = AgentDeliveryPayload {
                trace_id: crate::flow_trace::envelope_flow(svc, to_tab, &messenger.envelope),
                transport: QueuedAgentPayload::RoutedFrame {
                    opening_tag: frame.opening_tag,
                    body: frame.body,
                    suffix: frame.suffix,
                },
                observer,
                idempotency_key,
                expected_pty_id: Some(pty_id),
                expected_session_id: Some(expected_session_id.to_string()),
                expected_binding_nonce: Some(expected_binding_nonce.to_string()),
            };
            return svc.deliver_agent_payload(to_tab, payload, &text);
        }
        let (plugin_id, session_id) = svc
            .tab_chat_backing(to_tab)
            .ok_or_else(|| "agent_unavailable: planner has no chat delivery target".to_string())?;
        if session_id != expected_session_id {
            return Err(format!(
                "agent_unavailable: planner provider session rotated before send; expected={expected_session_id} observed={session_id}"
            ));
        }
        let delivery = Self::deliver_chatbackend_with_id(
            svc,
            to_tab,
            &plugin_id,
            &session_id,
            envelope,
            body,
            message_id,
        )?;
        let observed = {
            let mux = svc.mux.read();
            let resolved = mux.resolve_tab_id(to_tab);
            resolved.and_then(|tab_id| mux.get_tab(&tab_id)).map(|tab| {
                (
                    tab.agent_session_id.clone(),
                    tab.reported_session_binding_nonce.clone(),
                    tab.backing.clone(),
                )
            })
        };
        let still_bound = observed.as_ref().is_some_and(|(session, nonce, backing)| {
            planner_chatbackend_binding_matches(
                session.as_deref(),
                nonce.as_deref(),
                Some(backing),
                expected_session_id,
                expected_binding_nonce,
                &plugin_id,
            )
        });
        if !still_bound {
            let (observed_session, observed_nonce) = observed
                .map(|(session, nonce, _)| (session, nonce))
                .unwrap_or_default();
            return Err(format!(
                "delivery_uncertain: planner provider binding rotated after send started; expected_session={expected_session_id} expected_nonce={expected_binding_nonce} observed_session={} observed_nonce={}",
                observed_session.as_deref().unwrap_or("missing"),
                observed_nonce.as_deref().unwrap_or("missing")
            ));
        }
        Ok(delivery)
    }

    fn deliver_chatbackend(
        svc: &super::ServiceContext,
        to_tab: &str,
        plugin_id: &str,
        session_id: &str,
        envelope: MessageEnvelope,
        body: &str,
    ) -> Result<Delivery, String> {
        let message_id = format!("inbound-{}", uuid::Uuid::new_v4().simple());
        Self::deliver_chatbackend_with_id(
            svc, to_tab, plugin_id, session_id, envelope, body, message_id,
        )
    }

    fn deliver_chatbackend_with_id(
        svc: &super::ServiceContext,
        to_tab: &str,
        plugin_id: &str,
        session_id: &str,
        envelope: MessageEnvelope,
        body: &str,
        message_id: String,
    ) -> Result<Delivery, String> {
        let (msg, marked) = build_chatbackend_inbound(&envelope, body, message_id.clone());
        let trace_id = crate::flow_trace::envelope_flow(svc, to_tab, &envelope);

        let inserted = crate::service::tab_chat::chatbackend_history::ChatBackendHistory::shared(
            plugin_id, session_id,
        )
        .lock()
        .upsert(std::slice::from_ref(&msg));
        if inserted {
            if let Some(id) = trace_id.as_deref() {
                crate::flow_trace::record(
                    id,
                    "transcript.append",
                    to_tab,
                    plugin_id,
                    "inbound",
                    Some(&message_id),
                );
            }
            let (source_session_id, session_binding_nonce) = svc
                .mux
                .read()
                .get_tab(to_tab)
                .map(|tab| {
                    (
                        tab.agent_session_id.clone(),
                        tab.session_binding_nonce.clone(),
                    )
                })
                .unwrap_or((None, None));
            crate::service::tab_chat::structured_sink::emit_chat_msgs(
                &svc.app_handle,
                to_tab,
                std::slice::from_ref(&msg),
                None,
                source_session_id.as_deref(),
                session_binding_nonce.as_deref(),
            );
        }

        // The model gets the marker (sender + return address); the stored row carries only the clean body.
        if let Err(error) = submit_chatbackend_provider_input(&marked, |input| {
            svc.chatbackend_send(plugin_id, session_id, input)
        }) {
            if inserted {
                crate::service::tab_chat::chatbackend_history::ChatBackendHistory::shared(
                    plugin_id, session_id,
                )
                .lock()
                .remove(&message_id);
            }
            return Err(error);
        }
        Ok(Delivery::ChatBackend { message_id })
    }

    /// Render the opening `<domios>` tag for this envelope.
    pub fn build(&self) -> String {
        let mut attrs = vec![("from", self.envelope.kind.marker_kind().to_string())];
        if self.envelope.kind == MessageKind::System {
            if let Some(context) = self
                .envelope
                .context
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                attrs.push(("context", context.to_string()));
            }
            return domios_opening_tag(&attrs);
        }
        if self.envelope.kind == MessageKind::Reminder {
            return self.build_reminder();
        }
        if self.envelope.kind == MessageKind::Pebble {
            attrs.push(("source", clean_name(&self.envelope.from_label)));
            if let Some(plugin) = self
                .envelope
                .from_plugin
                .as_deref()
                .filter(|v| !v.is_empty())
            {
                attrs.push(("plugin", plugin.to_string()));
            }
            return domios_opening_tag(&attrs);
        }
        let tab = short_id(&self.envelope.from_tab);
        let base = clean_name(&self.envelope.from_label);
        // Peer-qualify the label for cross-mesh deliveries; skip `@peer` when no distinct source label exists.
        let label = match &self.envelope.peer {
            Some(peer) => {
                let peer_clean = clean_name(peer);
                if base.is_empty() || base == peer_clean {
                    peer_clean
                } else {
                    format!("{}@{}", base, peer_clean)
                }
            }
            None => base,
        };
        attrs.push(("label", label));
        attrs.push(("tab", tab.to_string()));
        if let Some(purpose) = self
            .envelope
            .purpose
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            attrs.push(("purpose", purpose.to_string()));
        }
        if self.envelope.relay_hops > 0 {
            attrs.push(("relay_hops", self.envelope.relay_hops.to_string()));
        }
        if let Some(model) = self
            .envelope
            .from_model
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            attrs.push(("model", model.to_string()));
        }
        if let Some(mesh) = self
            .envelope
            .from_mesh
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            attrs.push(("mesh", mesh.to_string()));
        }
        domios_opening_tag(&attrs)
    }

    fn build_reminder(&self) -> String {
        let label = clean_name(&self.envelope.from_label);
        let tab = short_id(&self.envelope.from_tab);
        let set_at = self.envelope.set_at.map(|v| v.to_string());
        let fire_at = self.envelope.fire_at.map(|v| v.to_string());
        let every_ms = self.envelope.every_ms.map(|v| v.to_string());
        let next_at = self.envelope.next_at_ms.map(|v| v.to_string());
        let skipped_ticks = self.envelope.skipped_ticks.map(|v| v.to_string());
        let mut attrs = vec![("from", "reminder".to_string())];
        for (key, value) in [
            ("label", Some(label)),
            ("tab", Some(tab.to_string())),
            (
                "model",
                self.envelope
                    .from_model
                    .as_deref()
                    .map(str::trim)
                    .map(str::to_string),
            ),
            ("set_at", set_at),
            ("fire_at", fire_at),
            ("id", self.envelope.id.clone()),
            ("every_ms", every_ms),
            ("next_at", next_at),
            ("skipped", skipped_ticks),
        ] {
            if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
                attrs.push((key, value));
            }
        }
        domios_opening_tag(&attrs)
    }

    /// Serialize the XML-tagged routed message, including the closing tag.
    pub fn wrap(&self, body: &str) -> String {
        self.routed_frame(body).to_message()
    }

    fn reply_hint(&self) -> Option<String> {
        if self.envelope.kind != MessageKind::Message || self.envelope.from_tab.is_empty() {
            return None;
        }
        let tab = short_id(&self.envelope.from_tab);
        if !safe_reply_token(tab) {
            return None;
        }
        if self.envelope.from_agent == Some(false) {
            return Some(Self::SHELL_REPLY_HINT.to_string());
        }
        let mut command = format!("{} \"YOUR REPLY\" --tab {}", Self::AGENT_REPLY_COMMAND, tab);
        if let Some(mesh) = self
            .envelope
            .from_mesh
            .as_deref()
            .map(str::trim)
            .filter(|mesh| !mesh.is_empty())
        {
            if !safe_reply_token(mesh) {
                return None;
            }
            command.push_str(" --mesh ");
            command.push_str(mesh);
        }
        Some(format!("{}{command}]", Self::AGENT_REPLY_HINT_PREFIX))
    }
}

fn pty_submit_entry(
    kind: MessageKind,
) -> crate::service::pty_io::submit_transaction::PtySubmitEntry {
    match kind {
        MessageKind::Report => crate::service::pty_io::submit_transaction::PtySubmitEntry::Report,
        MessageKind::Message
        | MessageKind::System
        | MessageKind::Reminder
        | MessageKind::Mesh
        | MessageKind::Telegram
        | MessageKind::Pebble => {
            crate::service::pty_io::submit_transaction::PtySubmitEntry::Messenger
        }
    }
}

fn domios_opening_tag(attrs: &[(impl AsRef<str>, String)]) -> String {
    let attrs = attrs
        .iter()
        .map(|(key, value)| (key.as_ref(), value.as_str()))
        .collect::<Vec<_>>();
    codeterm_types::domios::build_opening_tag(&attrs)
}

fn submit_chatbackend_provider_input<T>(
    marked: &str,
    submit: impl FnOnce(&str) -> Result<T, String>,
) -> Result<T, String> {
    submit(marked)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeliveryTransport {
    ChatBackend,
    Pty,
}

fn dispatch_delivery<T, E>(
    transport: DeliveryTransport,
    chat: impl FnOnce() -> Result<T, E>,
    pty: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    match transport {
        DeliveryTransport::ChatBackend => chat(),
        DeliveryTransport::Pty => pty(),
    }
}

fn planner_chatbackend_binding_matches(
    observed_session_id: Option<&str>,
    observed_binding_nonce: Option<&str>,
    observed_backing: Option<&crate::multiplexer::types::TabBacking>,
    expected_session_id: &str,
    expected_binding_nonce: &str,
    expected_plugin_id: &str,
) -> bool {
    observed_session_id == Some(expected_session_id)
        && observed_binding_nonce == Some(expected_binding_nonce)
        && matches!(
            observed_backing,
            Some(crate::multiplexer::types::TabBacking::ChatBackend {
                plugin_id,
                session_id,
            }) if plugin_id == expected_plugin_id && session_id == expected_session_id
        )
}

/// Build the `(stored_row, model_facing_string)` pair for a chatBackend inbound:
/// the stored `ChatMsg` carries the clean body + structured `from_*` fields, the
/// returned string is the marker-wrapped body the model sees. Pure for testability.
fn build_chatbackend_inbound(
    envelope: &MessageEnvelope,
    body: &str,
    message_id: String,
) -> (crate::agent::chat_types::ChatMsg, String) {
    let marked = inbound_model_context(envelope, body);
    let inbound_kind = envelope_inbound_kind(envelope);
    let msg = crate::agent::chat_types::ChatMsg {
        id: message_id,
        kind: "user".into(),
        content: body.to_string(),
        timestamp: Some(chrono::Utc::now().to_rfc3339()),
        from_label: Some(clean_name(&envelope.from_label)).filter(|s| !s.is_empty()),
        from_tab: Some(short_id(&envelope.from_tab).to_string()).filter(|s| !s.is_empty()),
        from_model: envelope.from_model.clone(),
        from_purpose: envelope.purpose.clone(),
        from_context: envelope.context.clone(),
        from_mesh: envelope.from_mesh.clone().or(envelope.peer.clone()),
        from_plugin: envelope.from_plugin.clone(),
        plugin_icon_svg: envelope.plugin_icon_svg.clone(),
        inbound_kind: Some(inbound_kind.to_string()),
        ..Default::default()
    };
    (msg, marked)
}

fn inbound_model_context(envelope: &MessageEnvelope, body: &str) -> String {
    let label = clean_name(&envelope.from_label);
    let mut attrs = vec![("from", envelope_inbound_kind(envelope).to_string())];
    for (key, value) in [
        ("label", Some(label)),
        ("tab", Some(short_id(&envelope.from_tab).to_string())),
        ("purpose", envelope.purpose.clone()),
        (
            "model",
            envelope
                .from_model
                .as_deref()
                .map(str::trim)
                .map(str::to_string),
        ),
        (
            "context",
            envelope
                .context
                .as_deref()
                .map(str::trim)
                .map(str::to_string),
        ),
        (
            "mesh",
            envelope
                .from_mesh
                .as_deref()
                .or(envelope.peer.as_deref())
                .map(str::trim)
                .map(str::to_string),
        ),
        ("plugin", envelope.from_plugin.clone()),
    ] {
        if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
            attrs.push((key, value));
        }
    }
    codeterm_types::domios::DomiosFrame {
        opening_tag: domios_opening_tag(&attrs),
        body: body.to_string(),
        suffix: String::new(),
    }
    .to_message()
}

fn envelope_inbound_kind(envelope: &MessageEnvelope) -> &'static str {
    if envelope.from_plugin.is_some() {
        "plugin"
    } else {
        envelope.kind.inbound_kind()
    }
}

/// Reminders and host events always mark agent input; other kinds keep the
/// self-send and empty-source guards. Shell input is never prefixed.
fn should_prefix_pty(
    kind: MessageKind,
    target_is_agent: bool,
    from_tab: &str,
    resolved_target: &str,
) -> bool {
    if !target_is_agent {
        return false;
    }
    if matches!(
        kind,
        MessageKind::Reminder | MessageKind::System | MessageKind::Pebble
    ) {
        return true;
    }
    !from_tab.is_empty() && from_tab != resolved_target
}

/// Build a complete sender-attributed task frame. `None` means no valid source route.
pub fn delivery_message(
    target_is_agent: bool,
    source_tab: &str,
    target_tab: &str,
    label: &str,
    model: Option<&str>,
    peer: Option<&str>,
    from_agent: bool,
    body: &str,
) -> Option<String> {
    if !target_is_agent || source_tab.is_empty() || source_tab == target_tab {
        return None;
    }
    let mut pfx = Messenger::message(label, source_tab).model(model.map(str::to_string));
    if let Some(p) = peer.filter(|p| !p.trim().is_empty()) {
        pfx = pfx.peer(Some(p.to_string())).mesh(Some(p.to_string()));
    }
    Some(pfx.from_agent(Some(from_agent)).wrap(body))
}

#[cfg(test)]
mod tests;
