use serde::{Deserialize, Serialize};

/// A workspace contains multiple tabs and tracks which tab is active.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub tabs: Vec<Tab>,
    pub active_tab_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    /// Protected workspaces cannot be closed via API or UI.
    #[serde(default)]
    pub protected: bool,
}

/// A tab holds exactly one pane.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tab {
    pub id: String,
    pub title: String,
    pub pane_id: String,
}

/// Legacy — kept only for old snapshot deserialization.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum Orientation {
    Horizontal,
    Vertical,
}

/// Agent role in the orchestration hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum AgentRole {
    /// Top-level orchestrator (depth 1). Can spawn Workers.
    Orchestrator,
    /// Worker (depth 2). Executes tasks, reports back. Cannot spawn.
    Worker,
}

// Keep backward-compatible aliases for deserialization
impl AgentRole {
    pub fn from_str_compat(s: &str) -> Option<Self> {
        match s {
            "Orchestrator" | "Conductor" => Some(Self::Orchestrator),
            "Worker" => Some(Self::Worker),
            _ => None,
        }
    }
}

/// An interactive prompt detected in agent output (e.g. permission request).
/// Provider-agnostic — any agent can have pending prompts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptOption {
    pub label: String,
    /// Value to send to PTY (e.g. "1", "y", "n")
    pub value: String,
}

/// A pending interactive prompt from an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingPrompt {
    pub question: String,
    pub options: Vec<PromptOption>,
}

/// A pane represents a single terminal or browser panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pane {
    pub id: String,
    /// Linked PTY ID, if this pane has a running terminal.
    pub pty_id: Option<String>,
    pub pane_type: PaneType,
    pub title: String,
    pub cwd: Option<String>,
    /// URL for browser panes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Agent type (e.g. "claude") — set by frontend when detected.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub agent_type: Option<String>,
    /// When true, only explicit unlock or agent exit can change `agent_type`.
    /// Set by spawn paths and input-parsed launch commands; blocks output-based
    /// re-detection so a user-launched provider can't be flipped by stray banners.
    #[serde(default)]
    pub agent_type_locked: bool,
    /// Timestamp when agent_type was set — grace period for idle-poll exit detection.
    #[serde(skip, default)]
    pub agent_set_at: Option<std::time::Instant>,
    /// True while launch_agent boots the agent TUI in this pane. A booting
    /// TUI silently swallows bracket-paste input, so sends wait this out.
    /// Runtime-only: never persisted (a restart clears any stale true).
    #[serde(skip, default)]
    pub agent_booting: bool,
    /// Agent session ID for resume (e.g. Claude Code session).
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        alias = "claude_session_id"
    )]
    pub agent_session_id: Option<String>,
    /// Human-readable session name emitted by the agent's TUI (e.g. Claude
    /// Code's "vivid-bhabha-1234" handle). Captured once per agent run via
    /// AgentProvider::extract_session_name. Searchable via `codeterm pane find`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub agent_session_name: Option<String>,
    /// Current model family on the live session ("Opus"/"Sonnet"/"Haiku").
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub agent_session_model: Option<String>,
    /// Orchestration role (orchestrator / lead / worker).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub role: Option<AgentRole>,
    /// Parent pane ID in the orchestration hierarchy.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub parent_pane_id: Option<String>,
    /// Whether this pane can spawn child agents.
    #[serde(default)]
    pub can_spawn: bool,
    /// Depth in hierarchy: 0=none, 1=orchestrator, 2=lead/worker, 3=worker.
    #[serde(default)]
    pub depth: u8,
    /// Report target pane ID — for cron agents that report without joining hierarchy.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub report_target_pane_id: Option<String>,
    /// Pane id of the caller that ran `agent spawn` (CODETERM_PANE_ID at spawn time).
    /// Independent of `parent_pane_id` (which controls the role hierarchy and is
    /// forbidden for Orchestrators). Used as a final fallback by report routing
    /// so a top-level orchestrator's report still reaches its spawning agent.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub spawned_by_pane_id: Option<String>,
    /// Title naming priority level.
    #[serde(default)]
    pub title_level: TitleLevel,
    /// Whether chat view overlay is active for this pane.
    #[serde(default)]
    pub chat_view: bool,
    /// Pending interactive prompt (permission, confirmation, etc.)
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pending_prompt: Option<PendingPrompt>,
    /// Auto-approve prompts (set when spawned with skip_permissions).
    /// When true, detected prompts are answered automatically instead of shown to user.
    #[serde(default)]
    pub auto_approve: bool,
    /// Protected panes cannot be closed via API or UI.
    #[serde(default)]
    pub protected: bool,
    /// Connected external services (e.g. ["telegram"]). Shown as icons next to pane title.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connected_services: Vec<String>,
    /// Hidden panes are excluded from default pane listings and TabBar.
    /// Used by graph-worker panes that live only in the memory panel.
    #[serde(default)]
    pub hidden: bool,
    /// Whether this pane should be rendered in the TabBar. Defaults to true.
    /// Graph-worker panes set this to false even when temporarily focused.
    #[serde(default = "default_show_in_tabbar")]
    pub show_in_tabbar: bool,
    /// Graph-worker id linking this pane to a GraphWorkerRegistry entry.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub worker_id: Option<String>,
    /// In-memory only — true when pane was deserialized from a session
    /// snapshot. Gates the spawn-fresh-agent path in commands::pty so it
    /// does NOT race the orchestrator's launch on freshly spawned panes.
    /// Cleared after the first successful spawn so a re-mount doesn't
    /// trigger another launch.
    #[serde(skip, default)]
    pub restored_from_snapshot: bool,
}

fn default_show_in_tabbar() -> bool {
    true
}

/// Title priority: higher levels cannot be overwritten by lower ones.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum TitleLevel {
    /// "Terminal" — initial default, lowest priority
    Default = 0,
    /// Auto-set by CWD/directory change (OSC 7)
    Auto = 1,
    /// Set by agent via OSC title (e.g. Claude sets task summary)
    Agent = 2,
    /// Manually set by user — highest priority, never overwritten
    User = 3,
}

impl Default for TitleLevel {
    fn default() -> Self {
        Self::Default
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum PaneType {
    #[default]
    Terminal,
    /// Legacy — old snapshots only.
    Browser,
}
