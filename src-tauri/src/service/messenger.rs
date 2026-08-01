//! Gateway for cross-tab / report / mesh deliveries. PTY-backed recipients get
//! the legacy text prefix read by the receiving agent and UI parser
//! (`src/lib/chatPrefixes.ts`). chatBackend recipients get structured host-side
//! transcript metadata instead.
//!
//!   -=-codeterm:tab label=<Name> tab=<id>[ model=<m>][ mesh=<peer>]-=-body
//!   -=-codeterm:report from=<Name> tab=<id> ...-=-body
//!
//! This wraps the unified `message_markers` frame so tab/report prefixes and
//! bridge attachment markers share one wire family.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MessageKind {
    /// A direct message from one tab (or peer) to another.
    #[default]
    Message,
    /// A hierarchy / cron report bubbling up to a parent or inbox.
    Report,
    /// A fired delayed reminder injected back into a tab.
    Reminder,
    Mesh,
    Telegram,
}

#[derive(Debug, Default, Clone)]
pub struct MessageEnvelope {
    pub kind: MessageKind,
    pub from_label: String,
    pub from_tab: String,
    pub from_model: Option<String>,
    pub from_mesh: Option<String>,
    pub peer: Option<String>,
    pub set_at: Option<u64>,
    pub fire_at: Option<u64>,
    pub id: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct Messenger {
    pub envelope: MessageEnvelope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    Pty { text: String },
    ChatBackend { message_id: String },
}

impl MessageKind {
    fn marker_kind(self) -> &'static str {
        match self {
            Self::Message => "tab",
            Self::Report => "report",
            Self::Reminder => "reminder",
            Self::Mesh => "mesh",
            Self::Telegram => "telegram",
        }
    }

    fn inbound_kind(self) -> &'static str {
        match self {
            Self::Message => "tab",
            Self::Report => "report",
            Self::Reminder => "reminder",
            Self::Mesh => "mesh",
            Self::Telegram => "telegram",
        }
    }
}

impl MessageEnvelope {
    /// CodeTerm's own sender identity — single source so no path hardcodes it.
    pub const CODETERM: &'static str = "CodeTerm";
    pub const CODETERM_TAB: &'static str = "system";

    /// A message FROM CodeTerm (renders "from CodeTerm") — role onboarding, etc.
    pub fn from_codeterm() -> Self {
        Self::tab(Self::CODETERM, Self::CODETERM_TAB)
    }

    /// A reminder FROM CodeTerm tagged with a context (nudges, prefs, qa, ...).
    pub fn codeterm_reminder(context: impl Into<String>) -> Self {
        Self::reminder(Self::CODETERM, context)
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

    pub fn model(mut self, model: Option<String>) -> Self {
        self.from_model = model.filter(|m| !m.trim().is_empty());
        self
    }

    pub fn mesh(mut self, mesh: Option<String>) -> Self {
        self.from_mesh = mesh.filter(|m| !m.trim().is_empty());
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

impl From<MessageEnvelope> for Messenger {
    fn from(envelope: MessageEnvelope) -> Self {
        Self { envelope }
    }
}

impl Messenger {
    pub fn message(from_name: impl Into<String>, from_tab: impl Into<String>) -> Self {
        MessageEnvelope::tab(from_name, from_tab).into()
    }

    /// Launch-task prefix marking input as coming FROM CodeTerm — prepended to a
    /// fresh spawn's bg_task (role onboarding), the single source for that marker.
    pub fn codeterm_prefix() -> String {
        Self::message(MessageEnvelope::CODETERM, MessageEnvelope::CODETERM_TAB).build()
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

    pub fn mesh(mut self, mesh: Option<String>) -> Self {
        self.envelope.from_mesh = mesh.filter(|m| !m.trim().is_empty());
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

    pub fn deliver(
        svc: &super::ServiceContext,
        to_tab: &str,
        envelope: MessageEnvelope,
        body: &str,
    ) -> Result<Delivery, String> {
        let kind = envelope.kind;
        let result = Self::deliver_inner(svc, to_tab, envelope, body);
        if result.is_ok() {
            svc.record_kpi_delivery(to_tab, kind, body.len());
        }
        result
    }

    fn deliver_inner(
        svc: &super::ServiceContext,
        to_tab: &str,
        envelope: MessageEnvelope,
        body: &str,
    ) -> Result<Delivery, String> {
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
            );
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
        let entry = pty_submit_entry(envelope.kind);
        let text = if should_prefix {
            Self::from(envelope).wrap(body)
        } else {
            body.to_string()
        };
        dispatch_delivery(
            DeliveryTransport::Pty,
            || unreachable!(),
            || {
                crate::service::pty_io::submit_transaction::submit_entry(entry, &text, |text| {
                    if pty_agent_target {
                        svc.send_to_agent_when_ready_detached(&resolved_target, text);
                    } else {
                        svc.send_text_detached(&resolved_target, text);
                    }
                    Ok::<(), String>(())
                })?;
                Ok(Delivery::Pty { text })
            },
        )
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
            let text = Self::from(envelope).wrap(body);
            crate::service::pty_io::submit_transaction::submit_entry(
                crate::service::pty_io::submit_transaction::PtySubmitEntry::Planner,
                &text,
                |text| {
                    svc.send_to_authenticated_agent_session(
                        to_tab,
                        expected_session_id,
                        expected_binding_nonce,
                        text,
                    )
                },
            )?;
            return Ok(Delivery::Pty { text });
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

        let inserted = crate::service::tab_chat::chatbackend_history::ChatBackendHistory::shared(
            plugin_id, session_id,
        )
        .lock()
        .upsert(std::slice::from_ref(&msg));
        if inserted {
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

    /// Render just the `[...]` prefix token (no trailing space/body).
    pub fn build(&self) -> String {
        if self.envelope.kind == MessageKind::Reminder {
            return self.build_reminder();
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
        let report = self.envelope.kind == MessageKind::Report;
        let name_key = if report { "from" } else { "label" };
        crate::message_markers::Marker::new(self.envelope.kind.marker_kind())
            .attr(name_key, &label)
            .attr("tab", tab)
            .attr_opt("model", self.envelope.from_model.as_deref().map(str::trim))
            .attr_opt("mesh", self.envelope.from_mesh.as_deref().map(str::trim))
            .build()
    }

    fn build_reminder(&self) -> String {
        let label = clean_name(&self.envelope.from_label);
        let tab = short_id(&self.envelope.from_tab);
        let set_at = self.envelope.set_at.map(|v| v.to_string());
        let fire_at = self.envelope.fire_at.map(|v| v.to_string());
        crate::message_markers::Marker::new("reminder")
            .attr_opt("from", Some(label.as_str()).filter(|s| !s.is_empty()))
            .attr_opt("tab", Some(tab).filter(|s| !s.is_empty()))
            .attr_opt("model", self.envelope.from_model.as_deref().map(str::trim))
            .attr_opt("set_at", set_at.as_deref())
            .attr_opt("fire_at", fire_at.as_deref())
            .attr_opt("id", self.envelope.id.as_deref())
            .build()
    }

    /// Prefix marker + the message body.
    pub fn wrap(&self, body: &str) -> String {
        let Some(reply_hint) = self.reply_hint() else {
            return format!("{}{}", self.build(), body);
        };
        format!("{}{}\n{}", self.build(), body, reply_hint)
    }

    fn reply_hint(&self) -> Option<String> {
        if self.envelope.kind != MessageKind::Message
            || self.envelope.from_tab.is_empty()
            || self.envelope.from_tab == MessageEnvelope::CODETERM_TAB
        {
            return None;
        }
        Some(
            crate::message_markers::Marker::new("reply")
                .attr("tab", short_id(&self.envelope.from_tab))
                .attr_opt("mesh", self.envelope.from_mesh.as_deref().map(str::trim))
                .build(),
        )
    }
}

fn pty_submit_entry(
    kind: MessageKind,
) -> crate::service::pty_io::submit_transaction::PtySubmitEntry {
    match kind {
        MessageKind::Report => crate::service::pty_io::submit_transaction::PtySubmitEntry::Report,
        MessageKind::Message
        | MessageKind::Reminder
        | MessageKind::Mesh
        | MessageKind::Telegram => {
            crate::service::pty_io::submit_transaction::PtySubmitEntry::Messenger
        }
    }
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
    let msg = crate::agent::chat_types::ChatMsg {
        id: message_id,
        kind: "user".into(),
        content: body.to_string(),
        timestamp: Some(chrono::Utc::now().to_rfc3339()),
        from_label: Some(clean_name(&envelope.from_label)).filter(|s| !s.is_empty()),
        from_tab: Some(short_id(&envelope.from_tab).to_string()).filter(|s| !s.is_empty()),
        from_model: envelope.from_model.clone(),
        from_mesh: envelope.from_mesh.clone().or(envelope.peer.clone()),
        inbound_kind: Some(envelope.kind.inbound_kind().to_string()),
        ..Default::default()
    };
    (msg, marked)
}

fn inbound_model_context(envelope: &MessageEnvelope, body: &str) -> String {
    let label = clean_name(&envelope.from_label);
    crate::message_markers::Marker::new("inbound")
        .attr("kind", envelope.kind.inbound_kind())
        .attr_opt("from_label", Some(label.as_str()).filter(|s| !s.is_empty()))
        .attr_opt(
            "from_tab",
            Some(short_id(&envelope.from_tab)).filter(|s| !s.is_empty()),
        )
        .attr_opt("from_model", envelope.from_model.as_deref().map(str::trim))
        .attr_opt(
            "from_mesh",
            envelope
                .from_mesh
                .as_deref()
                .or(envelope.peer.as_deref())
                .map(str::trim),
        )
        .payload(body)
        .build()
}

/// Whether the PTY recipient gets a `-=-codeterm:…-=-` source prefix. Shells never
/// do; a reminder always does for an agent (system event, must be distinguishable
/// from typed input) even self-addressed; every other kind keeps the self-send/empty-source guard.
fn should_prefix_pty(
    kind: MessageKind,
    target_is_agent: bool,
    from_tab: &str,
    resolved_target: &str,
) -> bool {
    if !target_is_agent {
        return false;
    }
    if kind == MessageKind::Reminder {
        return true;
    }
    !from_tab.is_empty() && from_tab != resolved_target
}

/// The canonical source-prefix decision for every agent-to-agent delivery path.
/// `None` for a shell recipient, self-send, or empty source. With `peer` set the
/// delivery crossed the mesh and the label is peer-qualified (`label@peer`).
pub fn delivery_prefix(
    target_is_agent: bool,
    source_tab: &str,
    target_tab: &str,
    label: &str,
    model: Option<&str>,
    peer: Option<&str>,
) -> Option<String> {
    if !target_is_agent || source_tab.is_empty() || source_tab == target_tab {
        return None;
    }
    let mut pfx = Messenger::message(label, source_tab).model(model.map(str::to_string));
    if let Some(p) = peer.filter(|p| !p.trim().is_empty()) {
        pfx = pfx.peer(Some(p.to_string())).mesh(Some(p.to_string()));
    }
    Some(pfx.build())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pty_envelope_uses_the_shared_submit_transaction() {
        assert_eq!(
            pty_submit_entry(MessageKind::Message),
            crate::service::pty_io::submit_transaction::PtySubmitEntry::Messenger
        );
        assert_eq!(
            pty_submit_entry(MessageKind::Reminder),
            crate::service::pty_io::submit_transaction::PtySubmitEntry::Messenger
        );
        assert_eq!(
            pty_submit_entry(MessageKind::Mesh),
            crate::service::pty_io::submit_transaction::PtySubmitEntry::Messenger
        );
        assert_eq!(
            pty_submit_entry(MessageKind::Telegram),
            crate::service::pty_io::submit_transaction::PtySubmitEntry::Messenger
        );
        assert_eq!(
            pty_submit_entry(MessageKind::Report),
            crate::service::pty_io::submit_transaction::PtySubmitEntry::Report
        );
    }

    #[test]
    fn bare_message_tags_codeterm() {
        let p = Messenger::message("Edison", "e8a0ac20-1111").build();
        assert_eq!(p, "-=-codeterm:tab label=Edison tab=e8a0ac20-=-");
    }

    #[test]
    fn enriched_message_orders_segments() {
        let p = Messenger::message("Edison", "e8a0ac20")
            .model(Some("opus".into()))
            .mesh(Some("Mac-Air".into()))
            .build();
        assert_eq!(
            p,
            "-=-codeterm:tab label=Edison tab=e8a0ac20 model=opus mesh=Mac-Air-=-"
        );
    }

    #[test]
    fn report_uses_report_lead() {
        let p = Messenger::report("Worker", "abc12345")
            .model(Some("sonnet".into()))
            .build();
        assert_eq!(
            p,
            "-=-codeterm:report from=Worker tab=abc12345 model=sonnet-=-"
        );
    }

    #[test]
    fn empty_model_and_mesh_are_dropped() {
        let p = Messenger::message("A", "deadbeef")
            .model(Some("  ".into()))
            .mesh(Some(String::new()))
            .build();
        assert_eq!(p, "-=-codeterm:tab label=A tab=deadbeef-=-");
    }

    #[test]
    fn brackets_in_name_are_stripped() {
        let p = Messenger::message("Main [dev]", "abcdef12").build();
        assert_eq!(p, "-=-codeterm:tab label=Main%20dev tab=abcdef12-=-");
    }

    #[test]
    fn wrap_appends_body() {
        let p = Messenger::message("Edison", "e8a0ac20");
        assert_eq!(
            p.wrap("hello"),
            "-=-codeterm:tab label=Edison tab=e8a0ac20-=-hello\n-=-codeterm:reply tab=e8a0ac20-=-"
        );
    }

    #[test]
    fn mesh_reply_hint_carries_the_return_peer() {
        let p = Messenger::message("Edison", "e8a0ac20").mesh(Some("BLACKSTAR".into()));
        assert_eq!(
            p.wrap("hello"),
            "-=-codeterm:tab label=Edison tab=e8a0ac20 mesh=BLACKSTAR-=-hello\n-=-codeterm:reply tab=e8a0ac20 mesh=BLACKSTAR-=-"
        );
    }

    #[test]
    fn codeterm_and_reports_do_not_request_a_reply() {
        assert!(!Messenger::from(MessageEnvelope::from_codeterm())
            .wrap("hello")
            .contains("codeterm:reply"));
        assert!(!Messenger::report("Worker", "abc12345")
            .wrap("done")
            .contains("codeterm:reply"));
    }

    #[test]
    fn inbound_model_context_uses_canonical_marker() {
        let envelope = MessageEnvelope::tab("Chat Backend", "abc12345-ffff")
            .model(Some("qwen".into()))
            .mesh(Some("mesh-a".into()));
        let marked = inbound_model_context(&envelope, "hello");
        let legacy = ["[CodeTerm", " inbound"].concat();

        assert!(!marked.contains(&legacy));
        let parsed = crate::message_markers::parse(&marked).expect("inbound marker parses");
        assert_eq!(parsed.kind, "inbound");
        assert_eq!(parsed.attr("kind"), Some("tab"));
        assert_eq!(parsed.attr("from_label"), Some("Chat Backend"));
        assert_eq!(parsed.attr("from_tab"), Some("abc12345"));
        assert_eq!(parsed.attr("from_model"), Some("qwen"));
        assert_eq!(parsed.attr("from_mesh"), Some("mesh-a"));
        assert_eq!(parsed.payload, "hello");
    }

    #[test]
    fn peer_qualifies_label() {
        let p = Messenger::message("Edison", "e8a0ac20")
            .peer(Some("BLACKSTAR".into()))
            .build();
        assert_eq!(p, "-=-codeterm:tab label=Edison@BLACKSTAR tab=e8a0ac20-=-");
    }

    #[test]
    fn peer_without_label_uses_peer_alone() {
        // No distinct source label → don't render the silly `PEER@PEER`.
        let p = Messenger::message("", "e8a0ac20")
            .peer(Some("BLACKSTAR".into()))
            .build();
        assert_eq!(p, "-=-codeterm:tab label=BLACKSTAR tab=e8a0ac20-=-");
    }

    #[test]
    fn reminder_emits_provenance_attrs() {
        let p = Messenger::reminder("Main [dev]", "e8a0ac20-1111")
            .model(Some("opus".into()))
            .set_at(1000)
            .fire_at(2000)
            .id(Some("dmsg-abc".into()))
            .build();
        assert_eq!(
            p,
            "-=-codeterm:reminder from=Main%20dev tab=e8a0ac20 model=opus set_at=1000 fire_at=2000 id=dmsg-abc-=-"
        );
    }

    #[test]
    fn reminder_drops_unknown_provenance() {
        let p = Messenger::reminder("", "").set_at(5).fire_at(9).build();
        assert_eq!(p, "-=-codeterm:reminder set_at=5 fire_at=9-=-");
    }

    #[test]
    fn reminder_wraps_body() {
        let p = Messenger::reminder("Main", "e8a0ac20")
            .set_at(1)
            .fire_at(2)
            .id(Some("dmsg-1".into()));
        assert_eq!(
            p.wrap("ping"),
            "-=-codeterm:reminder from=Main tab=e8a0ac20 set_at=1 fire_at=2 id=dmsg-1-=-ping"
        );
    }

    // --- delivery_prefix: one canonical decision per delivery path ---

    #[test]
    fn delivery_local_agent_recipient_gets_prefix() {
        let p = delivery_prefix(true, "e8a0ac20", "ce9bce56", "Edison", Some("opus"), None);
        assert_eq!(
            p.as_deref(),
            Some("-=-codeterm:tab label=Edison tab=e8a0ac20 model=opus-=-")
        );
    }

    #[test]
    fn delivery_shell_recipient_gets_none() {
        // Shells run bytes verbatim — no source tag, local or mesh.
        assert_eq!(
            delivery_prefix(false, "e8a0ac20", "ce9bce56", "Edison", None, None),
            None
        );
        assert_eq!(
            delivery_prefix(
                false,
                "e8a0ac20",
                "ce9bce56",
                "Edison",
                None,
                Some("BLACKSTAR")
            ),
            None
        );
    }

    #[test]
    fn delivery_mesh_agent_recipient_gets_peer_qualified_prefix() {
        let p = delivery_prefix(
            true,
            "e8a0ac20",
            "ce9bce56",
            "Edison",
            Some("sonnet"),
            Some("BLACKSTAR"),
        );
        assert_eq!(
            p.as_deref(),
            Some(
                "-=-codeterm:tab label=Edison@BLACKSTAR tab=e8a0ac20 model=sonnet mesh=BLACKSTAR-=-"
            )
        );
    }

    #[test]
    fn delivery_self_send_gets_none() {
        assert_eq!(
            delivery_prefix(true, "e8a0ac20", "e8a0ac20", "Edison", None, None),
            None
        );
    }

    #[test]
    fn delivery_empty_source_gets_none() {
        assert_eq!(
            delivery_prefix(true, "", "ce9bce56", "Edison", None, None),
            None
        );
    }

    #[test]
    fn reminder_inbound_context_is_canonical_with_sender_fields() {
        let envelope = MessageEnvelope::reminder("Main", "e8a0ac20-1111")
            .model(Some("opus".into()))
            .set_at(1000)
            .fire_at(2000)
            .id(Some("dmsg-1".into()));
        let ctx = inbound_model_context(&envelope, "ping");
        let parsed = crate::message_markers::parse(&ctx).expect("reminder inbound marker parses");
        let legacy = ["[CodeTerm", " inbound"].concat();

        assert!(!ctx.contains(&legacy));
        assert_eq!(parsed.kind, "inbound");
        assert_eq!(parsed.attr("kind"), Some("reminder"));
        assert_eq!(parsed.attr("from_label"), Some("Main"));
        assert_eq!(parsed.attr("from_tab"), Some("e8a0ac20"));
        assert_eq!(parsed.attr("from_model"), Some("opus"));
        assert_eq!(parsed.payload, "ping");
    }

    #[test]
    fn should_prefix_reminder_always_marks_agent_recipient() {
        // Self-addressed reminder (creator == target) still gets the marker.
        assert!(should_prefix_pty(
            MessageKind::Reminder,
            true,
            "e8a0ac20",
            "e8a0ac20"
        ));
        // Unknown-creator reminder still gets the marker.
        assert!(should_prefix_pty(
            MessageKind::Reminder,
            true,
            "",
            "e8a0ac20"
        ));
    }

    #[test]
    fn should_prefix_reminder_skips_shell_recipient() {
        // Shells run injected bytes verbatim — no marker, even for a reminder.
        assert!(!should_prefix_pty(
            MessageKind::Reminder,
            false,
            "abc",
            "e8a0ac20"
        ));
    }

    #[test]
    fn should_prefix_message_keeps_self_send_guard() {
        // A plain message to itself is NOT prefixed.
        assert!(!should_prefix_pty(
            MessageKind::Message,
            true,
            "e8a0ac20",
            "e8a0ac20"
        ));
        // Empty source → no prefix.
        assert!(!should_prefix_pty(
            MessageKind::Message,
            true,
            "",
            "e8a0ac20"
        ));
        // Distinct agent sender → prefix.
        assert!(should_prefix_pty(
            MessageKind::Message,
            true,
            "e8a0ac20",
            "ce9bce56"
        ));
    }

    #[test]
    fn chatbackend_inbound_stores_clean_body_model_gets_marker() {
        // Stored row must be the clean body (no inline marker); the marker goes only to the model-facing string.
        let envelope = MessageEnvelope::tab("Gauss", "abc12345-ffff")
            .model(Some("qwen".into()))
            .mesh(Some("Mac-Air".into()));
        let (msg, marked) = build_chatbackend_inbound(&envelope, "ship it", "inbound-x".into());

        assert_eq!(msg.content, "ship it", "stored row is the clean body");
        assert!(
            !msg.content.contains("-=-codeterm"),
            "no raw marker leaks into the rendered body"
        );
        assert_eq!(msg.kind, "user");
        assert_eq!(msg.from_label.as_deref(), Some("Gauss"));
        assert_eq!(msg.from_tab.as_deref(), Some("abc12345"));
        assert_eq!(msg.from_model.as_deref(), Some("qwen"));
        assert_eq!(msg.from_mesh.as_deref(), Some("Mac-Air"));
        assert_eq!(msg.inbound_kind.as_deref(), Some("tab"));

        // The model still learns the sender + return address via the marker.
        assert!(marked.contains("-=-codeterm:inbound"));
        assert!(marked.contains("from_label=Gauss"));
        assert!(marked.contains("ship it"));
    }

    #[test]
    fn chatbackend_planner_payload_reaches_provider_input_as_data() {
        let payload = "- audit `model`\n- <nonexistent>\n$env:X='y'; $(false)";
        let envelope = MessageEnvelope::codeterm_reminder("planner-request");
        let (_, marked) = build_chatbackend_inbound(&envelope, payload, "planner-request-1".into());
        let mut provider_input = Vec::new();
        submit_chatbackend_provider_input(&marked, |input| {
            provider_input.push(input.to_string());
            Ok(())
        })
        .unwrap();
        assert_eq!(provider_input.len(), 1);
        assert!(provider_input[0].contains(payload));
    }

    #[test]
    fn chat_backend_delivery_bypasses_pty_submit() {
        let mut provider_inputs = Vec::new();
        let mut pty_submits = 0;
        dispatch_delivery(
            DeliveryTransport::ChatBackend,
            || {
                submit_chatbackend_provider_input("hello", |input| {
                    provider_inputs.push(input.to_string());
                    Ok::<_, String>(())
                })
            },
            || {
                pty_submits += 1;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(provider_inputs, ["hello"]);
        assert_eq!(pty_submits, 0);
    }

    #[test]
    fn chatbackend_rotation_after_preflight_is_not_accepted_as_delivered() {
        let opened = crate::multiplexer::types::TabBacking::ChatBackend {
            plugin_id: "provider".into(),
            session_id: "opened-session".into(),
        };
        assert!(planner_chatbackend_binding_matches(
            Some("opened-session"),
            Some("opened-nonce"),
            Some(&opened),
            "opened-session",
            "opened-nonce",
            "provider",
        ));

        let rotated = crate::multiplexer::types::TabBacking::ChatBackend {
            plugin_id: "provider".into(),
            session_id: "rotated-session".into(),
        };
        assert!(!planner_chatbackend_binding_matches(
            Some("rotated-session"),
            Some("rotated-nonce"),
            Some(&rotated),
            "opened-session",
            "opened-nonce",
            "provider",
        ));
    }

    #[test]
    fn inbound_context_has_sender_with_canonical_marker() {
        let envelope = MessageEnvelope::tab("Edison", "e8a0ac20-1111")
            .model(Some("opus".into()))
            .mesh(Some("BLACKSTAR".into()));
        let ctx = inbound_model_context(&envelope, "hello");
        let parsed = crate::message_markers::parse(&ctx).expect("marker parses");
        let legacy = ["[CodeTerm", " inbound"].concat();

        assert!(!ctx.contains(&legacy));
        assert_eq!(parsed.kind, "inbound");
        assert_eq!(parsed.attr("kind"), Some("tab"));
        assert_eq!(parsed.attr("from_label"), Some("Edison"));
        assert_eq!(parsed.attr("from_tab"), Some("e8a0ac20"));
        assert_eq!(parsed.attr("from_model"), Some("opus"));
        assert_eq!(parsed.attr("from_mesh"), Some("BLACKSTAR"));
        assert_eq!(parsed.payload, "hello");
    }
}
