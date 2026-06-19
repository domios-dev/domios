//! One builder for cross-pane / report / mesh injection prefixes. Read by the
//! receiving agent and the UI parser (`src/lib/chatPrefixes.ts`).
//!
//!   -=-codeterm:pane label=<Name> pane=<id>[ model=<m>][ mesh=<peer>]-=-body
//!   -=-codeterm:report from=<Name> pane=<id> ...-=-body
//!
//! This wraps the unified `message_markers` frame so pane/report prefixes and
//! bridge attachment markers share one wire family.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CtxKind {
    /// A direct message from one pane (or peer) to another.
    #[default]
    Message,
    /// A hierarchy / cron report bubbling up to a parent or inbox.
    Report,
    /// A fired delayed reminder injected back into a pane.
    Reminder,
}

#[derive(Debug, Default, Clone)]
pub struct CtxPrefix {
    pub kind: CtxKind,
    pub from_name: String,
    pub from_pane: String,
    pub model: Option<String>,
    pub mesh: Option<String>,
    /// Originating peer for cross-mesh deliveries. When set, the rendered
    /// label is qualified as `from_name@peer` (e.g. `Edison@BLACKSTAR`) so the
    /// recipient can attribute the message to the source agent *and* its host.
    pub peer: Option<String>,
    /// Reminder: when the reminder was queued (ms since epoch).
    pub set_at: Option<u64>,
    /// Reminder: when the reminder was scheduled to fire (ms since epoch).
    pub fire_at: Option<u64>,
    /// Reminder: the originating `DelayedMessage` id.
    pub id: Option<String>,
}

fn short_id(id: &str) -> &str {
    id.split('-').next().unwrap_or(id)
}

/// Brackets would break the parser's anchoring; strip them from names.
fn clean_name(name: &str) -> String {
    name.chars()
        .filter(|c| *c != '[' && *c != ']')
        .collect::<String>()
        .trim()
        .to_string()
}

impl CtxPrefix {
    pub fn message(from_name: impl Into<String>, from_pane: impl Into<String>) -> Self {
        Self {
            kind: CtxKind::Message,
            from_name: from_name.into(),
            from_pane: from_pane.into(),
            ..Default::default()
        }
    }

    pub fn report(from_name: impl Into<String>, from_pane: impl Into<String>) -> Self {
        Self {
            kind: CtxKind::Report,
            from_name: from_name.into(),
            from_pane: from_pane.into(),
            ..Default::default()
        }
    }

    pub fn reminder(from_name: impl Into<String>, from_pane: impl Into<String>) -> Self {
        Self {
            kind: CtxKind::Reminder,
            from_name: from_name.into(),
            from_pane: from_pane.into(),
            ..Default::default()
        }
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

    pub fn model(mut self, model: Option<String>) -> Self {
        self.model = model.filter(|m| !m.trim().is_empty());
        self
    }

    pub fn mesh(mut self, mesh: Option<String>) -> Self {
        self.mesh = mesh.filter(|m| !m.trim().is_empty());
        self
    }

    pub fn peer(mut self, peer: Option<String>) -> Self {
        self.peer = peer.filter(|p| !p.trim().is_empty());
        self
    }

    /// Render just the `[...]` prefix token (no trailing space/body).
    pub fn build(&self) -> String {
        if self.kind == CtxKind::Reminder {
            return self.build_reminder();
        }
        let pane = short_id(&self.from_pane);
        let base = clean_name(&self.from_name);
        // Peer-qualify the label for cross-mesh deliveries. Skip the `@peer`
        // suffix when no distinct source label is known (avoids `PEER@PEER`).
        let label = match &self.peer {
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
        let report = self.kind == CtxKind::Report;
        let kind = if report { "report" } else { "pane" };
        let name_key = if report { "from" } else { "label" };
        crate::message_markers::Marker::new(kind)
            .attr(name_key, &label)
            .attr("pane", pane)
            .attr_opt("model", self.model.as_deref().map(str::trim))
            .attr_opt("mesh", self.mesh.as_deref().map(str::trim))
            .build()
    }

    fn build_reminder(&self) -> String {
        let label = clean_name(&self.from_name);
        let pane = short_id(&self.from_pane);
        let set_at = self.set_at.map(|v| v.to_string());
        let fire_at = self.fire_at.map(|v| v.to_string());
        crate::message_markers::Marker::new("reminder")
            .attr_opt("from", Some(label.as_str()).filter(|s| !s.is_empty()))
            .attr_opt("pane", Some(pane).filter(|s| !s.is_empty()))
            .attr_opt("model", self.model.as_deref().map(str::trim))
            .attr_opt("set_at", set_at.as_deref())
            .attr_opt("fire_at", fire_at.as_deref())
            .attr_opt("id", self.id.as_deref())
            .build()
    }

    /// Prefix marker + the message body.
    pub fn wrap(&self, body: &str) -> String {
        format!("{}{}", self.build(), body)
    }
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
    let mut pfx = CtxPrefix::message(label, source_pane).model(model.map(str::to_string));
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
        let p = CtxPrefix::message("Edison", "e8a0ac20-1111").build();
        assert_eq!(p, "-=-codeterm:pane label=Edison pane=e8a0ac20-=-");
    }

    #[test]
    fn enriched_message_orders_segments() {
        let p = CtxPrefix::message("Edison", "e8a0ac20")
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
        let p = CtxPrefix::report("Worker", "abc12345")
            .model(Some("sonnet".into()))
            .build();
        assert_eq!(
            p,
            "-=-codeterm:report from=Worker pane=abc12345 model=sonnet-=-"
        );
    }

    #[test]
    fn empty_model_and_mesh_are_dropped() {
        let p = CtxPrefix::message("A", "deadbeef")
            .model(Some("  ".into()))
            .mesh(Some(String::new()))
            .build();
        assert_eq!(p, "-=-codeterm:pane label=A pane=deadbeef-=-");
    }

    #[test]
    fn brackets_in_name_are_stripped() {
        let p = CtxPrefix::message("Main [dev]", "abcdef12").build();
        assert_eq!(p, "-=-codeterm:pane label=Main%20dev pane=abcdef12-=-");
    }

    #[test]
    fn wrap_appends_body() {
        let p = CtxPrefix::message("Edison", "e8a0ac20");
        assert_eq!(
            p.wrap("hello"),
            "-=-codeterm:pane label=Edison pane=e8a0ac20-=-hello"
        );
    }

    #[test]
    fn peer_qualifies_label() {
        let p = CtxPrefix::message("Edison", "e8a0ac20")
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
        let p = CtxPrefix::message("", "e8a0ac20")
            .peer(Some("BLACKSTAR".into()))
            .build();
        assert_eq!(p, "-=-codeterm:pane label=BLACKSTAR pane=e8a0ac20-=-");
    }

    #[test]
    fn reminder_emits_provenance_attrs() {
        let p = CtxPrefix::reminder("Main [dev]", "e8a0ac20-1111")
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
        let p = CtxPrefix::reminder("", "").set_at(5).fire_at(9).build();
        assert_eq!(p, "-=-codeterm:reminder set_at=5 fire_at=9-=-");
    }

    #[test]
    fn reminder_wraps_body() {
        let p = CtxPrefix::reminder("Main", "e8a0ac20")
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
}
