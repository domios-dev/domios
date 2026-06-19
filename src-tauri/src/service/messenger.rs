//! Gateway for cross-pane / report / mesh deliveries. PTY-backed recipients get
//! the legacy text prefix read by the receiving agent and UI parser
//! (`src/lib/chatPrefixes.ts`). chatBackend recipients get structured host-side
//! transcript metadata instead.
//!
//!   -=-codeterm:pane label=<Name> pane=<id>[ model=<m>][ mesh=<peer>]-=-body
//!   -=-codeterm:report from=<Name> pane=<id> ...-=-body
//!
//! This wraps the unified `message_markers` frame so pane/report prefixes and
//! bridge attachment markers share one wire family.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MessageKind {
    /// A direct message from one pane (or peer) to another.
    #[default]
    Message,
    /// A hierarchy / cron report bubbling up to a parent or inbox.
    Report,
    /// A fired delayed reminder injected back into a pane.
    Reminder,
    Mesh,
    Telegram,
}

#[derive(Debug, Default, Clone)]
pub struct MessageEnvelope {
    pub kind: MessageKind,
    pub from_label: String,
    pub from_pane: String,
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
            Self::Message => "pane",
            Self::Report => "report",
            Self::Reminder => "reminder",
            Self::Mesh => "mesh",
            Self::Telegram => "telegram",
        }
    }

    fn inbound_kind(self) -> &'static str {
        match self {
            Self::Message => "pane",
            Self::Report => "report",
            Self::Reminder => "reminder",
            Self::Mesh => "mesh",
            Self::Telegram => "telegram",
        }
    }
}

impl MessageEnvelope {
    pub fn pane(from_label: impl Into<String>, from_pane: impl Into<String>) -> Self {
        Self {
            kind: MessageKind::Message,
            from_label: from_label.into(),
            from_pane: from_pane.into(),
            ..Default::default()
        }
    }

    pub fn report(from_label: impl Into<String>, from_pane: impl Into<String>) -> Self {
        Self {
            kind: MessageKind::Report,
            from_label: from_label.into(),
            from_pane: from_pane.into(),
            ..Default::default()
        }
    }

    pub fn reminder(from_label: impl Into<String>, from_pane: impl Into<String>) -> Self {
        Self {
            kind: MessageKind::Reminder,
            from_label: from_label.into(),
            from_pane: from_pane.into(),
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
    pub fn message(from_name: impl Into<String>, from_pane: impl Into<String>) -> Self {
        MessageEnvelope::pane(from_name, from_pane).into()
    }

    pub fn report(from_name: impl Into<String>, from_pane: impl Into<String>) -> Self {
        MessageEnvelope::report(from_name, from_pane).into()
    }

    pub fn reminder(from_name: impl Into<String>, from_pane: impl Into<String>) -> Self {
        MessageEnvelope::reminder(from_name, from_pane).into()
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
        to_pane: &str,
        envelope: MessageEnvelope,
        body: &str,
    ) -> Result<Delivery, String> {
        if svc.pane_chat_backing(to_pane).is_some() {
            svc.chatbackend_reopen_if_restored(to_pane)?;
            let (plugin_id, session_id) = svc
                .pane_chat_backing(to_pane)
                .ok_or_else(|| "chatBackend pane lost its backing".to_string())?;
            return Self::deliver_chatbackend(
                svc,
                to_pane,
                &plugin_id,
                &session_id,
                envelope,
                body,
            );
        }

        let should_prefix = {
            let mux = svc.mux.write();
            let resolved_target = mux
                .resolve_pane_id(to_pane)
                .unwrap_or_else(|| to_pane.to_string());
            let target_is_agent = mux
                .get_pane(&resolved_target)
                .and_then(|p| p.agent_type.as_ref())
                .is_some();
            target_is_agent
                && !envelope.from_pane.is_empty()
                && envelope.from_pane != resolved_target
        };
        let text = if should_prefix {
            Self::from(envelope).wrap(body)
        } else {
            body.to_string()
        };
        svc.send_text(to_pane, &text, true)?;
        Ok(Delivery::Pty { text })
    }

    fn deliver_chatbackend(
        svc: &super::ServiceContext,
        to_pane: &str,
        plugin_id: &str,
        session_id: &str,
        envelope: MessageEnvelope,
        body: &str,
    ) -> Result<Delivery, String> {
        let message_id = format!("inbound-{}", uuid::Uuid::new_v4().simple());
        let msg = crate::agent::chat_types::ChatMsg {
            id: message_id.clone(),
            kind: "user".into(),
            content: body.to_string(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            from_label: Some(clean_name(&envelope.from_label)).filter(|s| !s.is_empty()),
            from_pane: Some(short_id(&envelope.from_pane).to_string()).filter(|s| !s.is_empty()),
            from_model: envelope.from_model.clone(),
            from_mesh: envelope.from_mesh.clone().or(envelope.peer.clone()),
            inbound_kind: Some(envelope.kind.inbound_kind().to_string()),
            ..Default::default()
        };

        let path =
            crate::service::pane_chat::chatbackend_history::session_path(plugin_id, session_id);
        let mut history =
            crate::service::pane_chat::chatbackend_history::ChatBackendHistory::open_at(path);
        history.upsert(std::slice::from_ref(&msg));
        crate::service::pane_chat::structured_sink::emit_chat_msgs(
            &svc.app_handle,
            to_pane,
            std::slice::from_ref(&msg),
            None,
        );

        let staged =
            svc.run_outbound_stages(to_pane, format!("{}\n{}", model_context(&envelope), body));
        svc.chatbackend_send(plugin_id, session_id, &staged)?;
        Ok(Delivery::ChatBackend { message_id })
    }

    /// Render just the `[...]` prefix token (no trailing space/body).
    pub fn build(&self) -> String {
        if self.envelope.kind == MessageKind::Reminder {
            return self.build_reminder();
        }
        let pane = short_id(&self.envelope.from_pane);
        let base = clean_name(&self.envelope.from_label);
        // Peer-qualify the label for cross-mesh deliveries. Skip the `@peer`
        // suffix when no distinct source label is known (avoids `PEER@PEER`).
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
            .attr("pane", pane)
            .attr_opt("model", self.envelope.from_model.as_deref().map(str::trim))
            .attr_opt("mesh", self.envelope.from_mesh.as_deref().map(str::trim))
            .build()
    }

    fn build_reminder(&self) -> String {
        let label = clean_name(&self.envelope.from_label);
        let pane = short_id(&self.envelope.from_pane);
        let set_at = self.envelope.set_at.map(|v| v.to_string());
        let fire_at = self.envelope.fire_at.map(|v| v.to_string());
        crate::message_markers::Marker::new("reminder")
            .attr_opt("from", Some(label.as_str()).filter(|s| !s.is_empty()))
            .attr_opt("pane", Some(pane).filter(|s| !s.is_empty()))
            .attr_opt("model", self.envelope.from_model.as_deref().map(str::trim))
            .attr_opt("set_at", set_at.as_deref())
            .attr_opt("fire_at", fire_at.as_deref())
            .attr_opt("id", self.envelope.id.as_deref())
            .build()
    }

    /// Prefix marker + the message body.
    pub fn wrap(&self, body: &str) -> String {
        format!("{}{}", self.build(), body)
    }
}

fn model_context(envelope: &MessageEnvelope) -> String {
    let mut parts = vec![
        format!("kind={}", envelope.kind.inbound_kind()),
        format!("from_label={}", clean_name(&envelope.from_label)),
        format!("from_pane={}", short_id(&envelope.from_pane)),
    ];
    if let Some(model) = envelope
        .from_model
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        parts.push(format!("from_model={}", model.trim()));
    }
    if let Some(mesh) = envelope
        .from_mesh
        .as_deref()
        .or(envelope.peer.as_deref())
        .filter(|s| !s.trim().is_empty())
    {
        parts.push(format!("from_mesh={}", mesh.trim()));
    }
    format!("[CodeTerm inbound {}]", parts.join(" "))
}

/// The single canonical decision for every agent-to-agent delivery path
/// (local socket send, HTTP send, mesh relay). Returns the source prefix
/// token to prepend, or `None` when no prefix should be applied.
///
/// Shells run bytes verbatim — `target_is_agent == false` always yields
/// `None`. A self-send (`source_pane == target_pane`) or an empty source
/// also yields `None`. When `peer` is set the delivery crossed the mesh and
/// the label is peer-qualified (`label@peer`, tagged `mesh=peer`).
pub fn delivery_prefix(
    target_is_agent: bool,
    source_pane: &str,
    target_pane: &str,
    label: &str,
    model: Option<&str>,
    peer: Option<&str>,
) -> Option<String> {
    if !target_is_agent || source_pane.is_empty() || source_pane == target_pane {
        return None;
    }
    let mut pfx = Messenger::message(label, source_pane).model(model.map(str::to_string));
    if let Some(p) = peer.filter(|p| !p.trim().is_empty()) {
        pfx = pfx.peer(Some(p.to_string())).mesh(Some(p.to_string()));
    }
    Some(pfx.build())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_message_tags_codeterm() {
        let p = Messenger::message("Edison", "e8a0ac20-1111").build();
        assert_eq!(p, "-=-codeterm:pane label=Edison pane=e8a0ac20-=-");
    }

    #[test]
    fn enriched_message_orders_segments() {
        let p = Messenger::message("Edison", "e8a0ac20")
            .model(Some("opus".into()))
            .mesh(Some("Mac-Air".into()))
            .build();
        assert_eq!(
            p,
            "-=-codeterm:pane label=Edison pane=e8a0ac20 model=opus mesh=Mac-Air-=-"
        );
    }

    #[test]
    fn report_uses_report_lead() {
        let p = Messenger::report("Worker", "abc12345")
            .model(Some("sonnet".into()))
            .build();
        assert_eq!(
            p,
            "-=-codeterm:report from=Worker pane=abc12345 model=sonnet-=-"
        );
    }

    #[test]
    fn empty_model_and_mesh_are_dropped() {
        let p = Messenger::message("A", "deadbeef")
            .model(Some("  ".into()))
            .mesh(Some(String::new()))
            .build();
        assert_eq!(p, "-=-codeterm:pane label=A pane=deadbeef-=-");
    }

    #[test]
    fn brackets_in_name_are_stripped() {
        let p = Messenger::message("Main [dev]", "abcdef12").build();
        assert_eq!(p, "-=-codeterm:pane label=Main%20dev pane=abcdef12-=-");
    }

    #[test]
    fn wrap_appends_body() {
        let p = Messenger::message("Edison", "e8a0ac20");
        assert_eq!(
            p.wrap("hello"),
            "-=-codeterm:pane label=Edison pane=e8a0ac20-=-hello"
        );
    }

    #[test]
    fn peer_qualifies_label() {
        let p = Messenger::message("Edison", "e8a0ac20")
            .peer(Some("BLACKSTAR".into()))
            .build();
        assert_eq!(
            p,
            "-=-codeterm:pane label=Edison@BLACKSTAR pane=e8a0ac20-=-"
        );
    }

    #[test]
    fn peer_without_label_uses_peer_alone() {
        // No distinct source label → don't render the silly `PEER@PEER`.
        let p = Messenger::message("", "e8a0ac20")
            .peer(Some("BLACKSTAR".into()))
            .build();
        assert_eq!(p, "-=-codeterm:pane label=BLACKSTAR pane=e8a0ac20-=-");
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
            "-=-codeterm:reminder from=Main%20dev pane=e8a0ac20 model=opus set_at=1000 fire_at=2000 id=dmsg-abc-=-"
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
            "-=-codeterm:reminder from=Main pane=e8a0ac20 set_at=1 fire_at=2 id=dmsg-1-=-ping"
        );
    }

    // --- delivery_prefix: one canonical decision per delivery path ---

    #[test]
    fn delivery_local_agent_recipient_gets_prefix() {
        let p = delivery_prefix(true, "e8a0ac20", "ce9bce56", "Edison", Some("opus"), None);
        assert_eq!(
            p.as_deref(),
            Some("-=-codeterm:pane label=Edison pane=e8a0ac20 model=opus-=-")
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
                "-=-codeterm:pane label=Edison@BLACKSTAR pane=e8a0ac20 model=sonnet mesh=BLACKSTAR-=-"
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
    fn model_context_has_sender_without_legacy_marker() {
        let envelope = MessageEnvelope::pane("Edison", "e8a0ac20-1111")
            .model(Some("opus".into()))
            .mesh(Some("BLACKSTAR".into()));
        let ctx = model_context(&envelope);
        assert_eq!(
            ctx,
            "[CodeTerm inbound kind=pane from_label=Edison from_pane=e8a0ac20 from_model=opus from_mesh=BLACKSTAR]"
        );
        assert!(!ctx.contains("-=-codeterm"));
    }
}
