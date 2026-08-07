//! Opt-in Telegram push channel: a closed category set, clock-free coalescing
//! keys, and length-capped rendering over daemon-resolved provenance.

use super::store::NotificationProvenance;
use codeterm_types::config::schema::TelegramNotifyConfig;

/// The complete set of push-eligible categories. Routine progress has no
/// variant, so it is unsendable rather than merely undelivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PushCategory {
    Milestone,
    Blocker,
    Failure,
}

/// Wire values, in the same order as the CLI `value_enum`.
pub const PUSH_CATEGORY_VALUES: [&str; 3] = ["milestone", "blocker", "failure"];

impl PushCategory {
    /// Unknown input is a rejection, never a pass-through string.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "milestone" => Some(Self::Milestone),
            "blocker" => Some(Self::Blocker),
            "failure" => Some(Self::Failure),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Milestone => "milestone",
            Self::Blocker => "blocker",
            Self::Failure => "failure",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Milestone => "Milestone",
            Self::Blocker => "Needs you",
            Self::Failure => "Failed",
        }
    }
}

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
pub const MAX_BODY_CHARS: usize = 320;
pub const MAX_SOURCE_CHARS: usize = 48;
pub const MAX_PUSH_CHARS: usize = 500;

/// Truncate on a char boundary. Never rejects: rejection teaches an agent to
/// retry in smaller pieces, and retry is the spam vector.
fn cap(text: &str, max: usize) -> String {
    let mut capped: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        capped.push('…');
    }
    capped
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Render a push body. Reads only the title, the agent body, and the resolved
/// provenance labels — never cwd, prompt text, task text, logs, or ids.
pub fn render_push(
    category: PushCategory,
    title: &str,
    body: Option<&str>,
    provenance: &NotificationProvenance,
) -> String {
    let mut lines = vec![format!(
        "[{}] {}",
        category.label(),
        cap(&collapse(title), MAX_TITLE_CHARS)
    )];
    if let Some(body) = body.map(collapse).filter(|b| !b.is_empty()) {
        lines.push(cap(&body, MAX_BODY_CHARS));
    }
    if let Some(source) = render_source(provenance) {
        lines.push(format!("via {source}"));
    }
    cap(&lines.join("\n"), MAX_PUSH_CHARS)
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

/// The explicit audience, distinct from `telegram_allowed_users`. An entry that
/// is not a chat id is dropped rather than broadcast to.
pub fn resolve_audience(cfg: &TelegramNotifyConfig) -> Vec<i64> {
    if !cfg.enabled {
        return Vec::new();
    }
    let mut chat_ids: Vec<i64> = Vec::new();
    for candidate in &cfg.recipients {
        if let Ok(chat_id) = candidate.trim().parse::<i64>() {
            if !chat_ids.contains(&chat_id) {
                chat_ids.push(chat_id);
            }
        }
    }
    chat_ids
}

#[cfg(test)]
mod telegram_render {
    use super::*;

    fn provenance() -> NotificationProvenance {
        NotificationProvenance {
            tab_id: Some("tab_7f21c0de".into()),
            tab_title: Some("Worker lane".into()),
            workspace_id: Some("ws_lanes".into()),
            peer_name: None,
        }
    }

    #[test]
    fn telegram_render_truncates_oversized_agent_text() {
        let oversized_title = "T".repeat(MAX_TITLE_CHARS + 40);
        let oversized_body = "B".repeat(MAX_BODY_CHARS + 500);
        assert!(oversized_title.chars().count() > MAX_TITLE_CHARS);
        assert!(oversized_body.chars().count() > MAX_BODY_CHARS);

        let rendered = render_push(
            PushCategory::Failure,
            &oversized_title,
            Some(&oversized_body),
            &provenance(),
        );

        assert!(
            !rendered.is_empty(),
            "oversized input truncates rather than rejecting"
        );
        assert!(
            rendered.chars().count() <= MAX_PUSH_CHARS + 1,
            "rendered push stays within the message cap (+1 for the ellipsis)"
        );
        assert_eq!(
            rendered.matches('T').count(),
            MAX_TITLE_CHARS,
            "the title keeps exactly the cap constant's worth of characters"
        );
        assert!(rendered.contains('…'), "truncation is marked, not silent");
    }

    #[test]
    fn multibyte_truncation_stays_on_char_boundaries() {
        let title = "я".repeat(MAX_TITLE_CHARS + 10);
        let rendered = render_push(PushCategory::Milestone, &title, None, &provenance());
        assert_eq!(rendered.matches('я').count(), MAX_TITLE_CHARS);
    }

    #[test]
    fn rendered_push_never_carries_ids_or_workspace() {
        let rendered = render_push(
            PushCategory::Blocker,
            "approval needed",
            Some("continue with the destructive step?"),
            &provenance(),
        );
        assert!(!rendered.contains("tab_7f21c0de"), "full ids are excluded");
        assert!(!rendered.contains("ws_lanes"), "workspace ids are excluded");
        assert!(rendered.contains("via Worker lane"));
    }

    #[test]
    fn progress_is_not_a_parseable_category() {
        assert_eq!(PushCategory::parse("progress"), None);
        assert_eq!(PushCategory::parse("info"), None);
        assert_eq!(PushCategory::parse(""), None);
        for value in PUSH_CATEGORY_VALUES {
            assert!(
                PushCategory::parse(value).is_some(),
                "{value} is one of the three expressible categories"
            );
        }
    }

    #[test]
    fn audience_is_explicit_and_ignores_unparseable_entries() {
        let mut cfg = TelegramNotifyConfig {
            enabled: true,
            recipients: vec!["100000001".into(), "not-a-chat-id".into(), "".into()],
            per_scope_limit: 8,
        };
        assert_eq!(resolve_audience(&cfg), vec![100000001]);

        cfg.enabled = false;
        assert!(
            resolve_audience(&cfg).is_empty(),
            "opt-out yields no audience even with recipients configured"
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
