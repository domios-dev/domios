//! The push leg shared by every channel: the closed level set, clock-free
//! coalescing keys, and rendering over daemon-resolved provenance, bounded by
//! whichever channel is carrying the message. Which channels exist is
//! `notification::channels`; this module is what all of them render and scope
//! through.

use super::store::NotificationProvenance;
use codeterm_types::config::schema::PushChannelConfig;
pub use codeterm_types::notification::NotifyLevel;

/// Coalescing scope for a caller the daemon could not attribute to a tab or a
/// peer. One shared bucket, so the lifetime ceiling still binds.
pub const UNATTRIBUTED_SCOPE: &str = "<unattributed>";

pub fn tab_scope(tab_id: &str) -> String {
    format!("tab:{tab_id}")
}

/// Scope key from daemon-resolved provenance only — never a caller claim.
pub fn scope_key(provenance: &NotificationProvenance) -> String {
    if let Some(tab_id) = provenance.tab_id.as_deref() {
        return tab_scope(tab_id);
    }
    if let Some(peer) = provenance.peer_name.as_deref() {
        return format!("peer:{peer}");
    }
    UNATTRIBUTED_SCOPE.to_string()
}

/// An absent event key derives one from the caller's own identity, so dedup is
/// never opt-in and cannot be defeated by omitting the key.
pub fn event_key(supplied: Option<&str>, title: &str) -> String {
    match supplied.map(str::trim).filter(|key| !key.is_empty()) {
        Some(key) => format!("caller:{key}"),
        None => format!("derived:{}", title.trim()),
    }
}

pub const MAX_TITLE_CHARS: usize = 100;
pub const MAX_SOURCE_CHARS: usize = 48;

/// What a cut body says instead of trailing off. It points at the bell, which
/// holds the whole text and is reachable from the phone the push arrived on; a
/// filesystem path is not.
pub const OVERFLOW_NOTE: &str = "… truncated — the full text is in the bell";

/// Room the assembly spends on everything that is not the body: the
/// level-prefixed title line, the provenance line, their separators, and the
/// overflow marker.
const NON_BODY_RESERVE: usize = MAX_TITLE_CHARS + MAX_SOURCE_CHARS + 128;

/// The length budget of one channel's own transport. Core owns the shape of the
/// split; the ceiling itself is stated by the channel that has to carry the
/// message, so no single constant decides it for every channel at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PushBounds {
    pub title: usize,
    pub body: usize,
    pub total: usize,
}

impl PushBounds {
    /// Split one transport's ceiling into the parts of a push.
    pub const fn for_ceiling(total: usize) -> Self {
        Self {
            title: MAX_TITLE_CHARS,
            body: total.saturating_sub(NON_BODY_RESERVE),
            total,
        }
    }

    /// The same budget with room set aside for text a caller appends after the
    /// assembly, so the addition cannot push the message past the ceiling.
    pub fn reserving(self, extra: usize) -> Self {
        Self {
            title: self.title,
            body: self.body.saturating_sub(extra),
            total: self.total.saturating_sub(extra),
        }
    }
}

/// The material one push renders from, held unrendered so that each channel
/// renders it at its own ceiling rather than inheriting someone else's.
#[derive(Debug, Clone)]
pub struct PushContent {
    pub level: NotifyLevel,
    pub title: String,
    pub body: Option<String>,
    pub provenance: NotificationProvenance,
}

impl PushContent {
    pub fn render(&self, bounds: PushBounds) -> String {
        render_push(
            self.level,
            &self.title,
            self.body.as_deref(),
            &self.provenance,
            bounds,
        )
    }
}

/// Truncate on a char boundary. Never rejects: rejection teaches an agent to
/// retry in smaller pieces, and retry is the spam vector.
fn cap(text: &str, max: usize) -> String {
    let mut capped: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        capped.push('…');
    }
    capped
}

/// Truncate on a char boundary and say where the remainder can be read, inside
/// the same budget rather than past it. Never rejects, for the same reason
/// [`cap`] does not.
fn cap_and_tell(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let keep = max.saturating_sub(OVERFLOW_NOTE.chars().count());
    let kept: String = text.chars().take(keep).collect();
    format!("{}{OVERFLOW_NOTE}", kept.trim_end())
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Tidy whitespace without flattening the shape a human wrote. Trailing spaces
/// go and runs of blank lines collapse to one; a paragraph break survives,
/// because markdown is defined by line structure and not by spacing.
fn tidy(text: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() && lines.last().is_some_and(|last| last.is_empty()) {
            continue;
        }
        lines.push(line);
    }
    lines.join("\n").trim().to_string()
}

/// Render a push body at one channel's bound. Reads only the title, the agent
/// body, and the resolved provenance labels — never cwd, prompt text, task
/// text, logs, or ids. A wider bound is more room for those three, never
/// permission to read a fourth.
pub fn render_push(
    level: NotifyLevel,
    title: &str,
    body: Option<&str>,
    provenance: &NotificationProvenance,
    bounds: PushBounds,
) -> String {
    let mut lines = vec![format!(
        "[{}] {}",
        level.label(),
        cap(&collapse(title), bounds.title)
    )];
    if let Some(body) = body.map(tidy).filter(|b| !b.is_empty()) {
        lines.push(cap_and_tell(&body, bounds.body));
    }
    if let Some(source) = render_source(provenance) {
        lines.push(format!("via {source}"));
    }
    cap_and_tell(&lines.join("\n"), bounds.total)
}

fn render_source(provenance: &NotificationProvenance) -> Option<String> {
    let label = provenance
        .tab_title
        .as_deref()
        .or(provenance.peer_name.as_deref())
        .map(collapse)
        .filter(|label| !label.is_empty())?;
    Some(cap(&label, MAX_SOURCE_CHARS))
}

/// Whether the settings control may be offered at all. An empty inbound allow
/// list means nobody is on the other end of the bridge.
pub fn control_available(bridge_active: bool, allow_list_len: usize) -> bool {
    bridge_active && allow_list_len > 0
}

/// The explicit audience of one channel: exactly the recipients its own surface
/// stored, trimmed and deduplicated. An empty audience is nobody — it is
/// deliberately not that integration's inbound admission list, so clearing the
/// selection stops every push instead of widening it to everyone admitted.
/// Whether a level reaches this channel at all is the grid column, not this.
pub fn resolve_audience(cfg: &PushChannelConfig) -> Vec<String> {
    let mut recipients: Vec<String> = Vec::new();
    for candidate in &cfg.recipients {
        let entry = candidate.trim();
        if entry.is_empty() || recipients.iter().any(|kept| kept == entry) {
            continue;
        }
        recipients.push(entry.to_string());
    }
    recipients
}

#[cfg(test)]
mod push_body;

#[cfg(test)]
mod push_render {
    use super::*;

    const NARROW: PushBounds = PushBounds::for_ceiling(500);

    fn provenance() -> NotificationProvenance {
        NotificationProvenance {
            tab_id: Some("tab_7f21c0de".into()),
            tab_title: Some("Worker lane".into()),
            workspace_id: Some("ws_lanes".into()),
            peer_name: None,
        }
    }

    #[test]
    fn push_render_truncates_oversized_agent_text() {
        let oversized_title = "T".repeat(MAX_TITLE_CHARS + 40);
        let oversized_body = "B".repeat(NARROW.body + 500);
        assert!(oversized_title.chars().count() > NARROW.title);
        assert!(oversized_body.chars().count() > NARROW.body);

        let rendered = render_push(
            NotifyLevel::Failure,
            &oversized_title,
            Some(&oversized_body),
            &provenance(),
            NARROW,
        );

        assert!(
            !rendered.is_empty(),
            "oversized input truncates rather than rejecting"
        );
        assert!(
            rendered.chars().count() <= NARROW.total,
            "rendered push stays within the channel's own message cap"
        );
        assert_eq!(
            rendered.matches('T').count(),
            NARROW.title,
            "the title keeps exactly the bound's worth of characters"
        );
        assert!(rendered.contains('…'), "truncation is marked, not silent");
    }

    #[test]
    fn multibyte_truncation_stays_on_char_boundaries() {
        let title = "я".repeat(MAX_TITLE_CHARS + 10);
        let rendered = render_push(NotifyLevel::Verbose, &title, None, &provenance(), NARROW);
        assert_eq!(rendered.matches('я').count(), MAX_TITLE_CHARS);
    }

    #[test]
    fn rendered_push_never_carries_ids_or_workspace() {
        let rendered = render_push(
            NotifyLevel::ActionNeeded,
            "approval needed",
            Some("continue with the destructive step?"),
            &provenance(),
            NARROW,
        );
        assert!(!rendered.contains("tab_7f21c0de"), "full ids are excluded");
        assert!(!rendered.contains("ws_lanes"), "workspace ids are excluded");
        assert!(rendered.contains("via Worker lane"));
    }

    #[test]
    fn notify_level_progress_is_not_a_parseable_level() {
        assert_eq!(NotifyLevel::parse("progress"), None);
        assert_eq!(NotifyLevel::parse("info"), None);
        assert_eq!(NotifyLevel::parse(""), None);
        for value in NotifyLevel::wire_values() {
            assert!(
                NotifyLevel::parse(value).is_some(),
                "{value} is one of the three expressible levels"
            );
        }
    }

    #[test]
    fn channel_audience_is_exactly_the_stored_selection() {
        let cfg = PushChannelConfig {
            recipients: vec![
                " 100000001 ".into(),
                "100000001".into(),
                "".into(),
                "100000002".into(),
            ],
            per_scope_limit: 8,
        };
        assert_eq!(
            resolve_audience(&cfg),
            vec!["100000001".to_string(), "100000002".to_string()],
            "the audience is the selection, trimmed and deduplicated, and nothing more"
        );

        assert!(
            resolve_audience(&PushChannelConfig::default()).is_empty(),
            "an unchosen audience is nobody; there is no second field to consult"
        );
    }

    #[test]
    fn control_is_unavailable_without_a_live_bridge_or_an_allow_list() {
        assert!(!control_available(false, 3));
        assert!(!control_available(true, 0));
        assert!(control_available(true, 1));
    }

    #[test]
    fn scope_and_event_keys_fall_back_to_derived_identities() {
        let unattributed = NotificationProvenance::default();
        assert_eq!(scope_key(&unattributed), UNATTRIBUTED_SCOPE);
        assert_eq!(scope_key(&provenance()), "tab:tab_7f21c0de");

        assert_eq!(event_key(Some("build-42"), "any title"), "caller:build-42");
        assert_eq!(event_key(Some("   "), "any title"), "derived:any title");
        assert_eq!(event_key(None, "  any title "), "derived:any title");
    }
}
