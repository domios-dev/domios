//! Who a bridged message is from, rendered once for every surface carrying it.

use codeterm_types::agent::role_kind::builtins;

pub const HEADER_SEPARATOR: &str = " · ";
/// Telegram's ceiling for a forum topic name.
pub const TOPIC_NAME_MAX_CHARS: usize = 128;
const SUFFIX_CHARS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SenderRole {
    Orchestrator,
    Worker,
    Planner,
    General,
}

impl SenderRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Orchestrator => "orchestrator",
            Self::Worker => "worker",
            Self::Planner => "planner",
            Self::General => "general",
        }
    }

    /// A roleless or unrecognised tab coordinates nothing, so it reads as `worker`.
    pub fn classify(is_general: bool, role_profile: Option<&str>) -> Self {
        if is_general {
            return Self::General;
        }
        match role_profile.map(str::trim) {
            Some(builtins::GENERAL_AGENT) => Self::General,
            Some(builtins::ORCHESTRATOR) => Self::Orchestrator,
            Some(builtins::PLANNER) => Self::Planner,
            _ => Self::Worker,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SenderIdentity {
    pub tab_id: String,
    pub title: String,
    pub role: SenderRole,
    pub workspace: String,
    pub machine: String,
}

impl SenderIdentity {
    pub fn header(&self) -> String {
        let title = display_title(&self.title, &self.tab_id);
        [
            title.as_str(),
            self.role.as_str(),
            self.workspace.trim(),
            self.machine.trim(),
        ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(HEADER_SEPARATOR)
    }
}

/// What the daemon knows about live tabs, narrowed to what threads need.
pub trait ThreadTabs: Send + Sync {
    fn sender_identity(&self, tab_id: &str) -> Option<SenderIdentity>;
    /// `(tab_id, title)` for every live tab.
    fn live_tab_titles(&self) -> Vec<(String, String)>;
    fn is_live_tab(&self, tab_id: &str) -> bool;
    fn is_visible_tab(&self, tab_id: &str) -> bool;
    fn is_general_tab(&self, tab_id: &str) -> bool;
}

pub fn display_title(title: &str, tab_id: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        format!("tab {tab_id}")
    } else {
        title.to_string()
    }
}

pub(crate) fn truncate_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// Exactly the tab title, suffixed from the tab id only while another live tab
/// shares that title, so the name stays deterministic for the same tab.
pub fn topic_name(title: &str, tab_id: &str, live_tabs: &[(String, String)]) -> String {
    let base = display_title(title, tab_id);
    let shared = live_tabs.iter().any(|(other_id, other_title)| {
        other_id != tab_id && display_title(other_title, other_id) == base
    });
    if !shared {
        return truncate_chars(&base, TOPIC_NAME_MAX_CHARS);
    }
    let tail: String = tab_id
        .chars()
        .rev()
        .take(SUFFIX_CHARS)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    // The tail distinguishes ids that share a prefix such as `tab_`.
    let suffix = format!(" #{tail}");
    let room = TOPIC_NAME_MAX_CHARS.saturating_sub(suffix.chars().count());
    format!("{}{suffix}", truncate_chars(&base, room))
}
