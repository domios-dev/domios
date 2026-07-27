use serde::{Deserialize, Serialize};

pub use codeterm_types::pty::{AgentRole, PendingPrompt, PromptOption, PromptSection};

/// A workspace contains multiple tabs and tracks which tab is active.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub tabs: Vec<String>,
    pub active_tab_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    /// Protected workspaces cannot be closed via API or UI.
    #[serde(default)]
    pub protected: bool,
    /// Sibling ordinal among workspaces. Only relative order within the group
    /// matters; the frontend sorts by it. Legacy snapshots default to 0.
    #[serde(default)]
    pub sort_order: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelIdentityMismatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planned_model: Option<String>,
    pub requested_model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_resolved_model: Option<String>,
    pub actual_model: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelFallbackNotice {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planned_model: Option<String>,
    pub requested_model: String,
    pub provider_resolved_model: String,
    #[serde(default)]
    pub actual_model: Option<String>,
    pub reason: String,
}

/// A closed tab retained for browser-style reopen. Captures just enough to
/// re-materialize the tab with the SAME backing — the `backing` reuses the
/// existing `TabBacking` serialization, and `agent_type` / `agent_session_id`
/// carry the PTY-agent identity that lives outside `backing` so a reopened
/// agent tab can resume its session (or launch fresh if it's gone). Persisted
/// as one row in the sqlite closed-history store (`state::closed`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClosedTab {
    pub tab_id: String,
    /// Original parent tab id — preserves the orchestration hierarchy so a
    /// cascade reopen can relink the subtree.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub parent_tab_id: Option<String>,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
    #[serde(default)]
    pub backing: TabBacking,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub agent_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub agent_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub role_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub model: Option<String>,
    #[serde(default)]
    pub chat_view: bool,
    pub closed_at: u64,
    /// Workspace the tab belonged to when it was closed. Empty string on old
    /// records (pre-field snapshots) — treat as "unknown workspace".
    #[serde(default)]
    pub workspace_id: String,
    /// Human-readable workspace name for display in the sidebar list.
    #[serde(default)]
    pub workspace_name: String,
}

impl From<&Tab> for ClosedTab {
    /// Derive the closed record from a live tab. Captures every tab-owned
    /// field in ONE place so adding a `Tab` field can't silently desync the
    /// close path. The owning workspace + tab title/description are overlaid by
    /// the close path (they aren't tab-local).
    fn from(tab: &Tab) -> Self {
        ClosedTab {
            tab_id: tab.id.clone(),
            parent_tab_id: tab.parent_tab_id.clone(),
            title: tab.title.clone(),
            description: tab.description.clone(),
            backing: tab.backing.clone(),
            cwd: tab.cwd.clone(),
            agent_type: tab.agent_type.clone(),
            agent_session_id: tab.agent_session_id.clone(),
            role_profile: tab.role_profile.clone(),
            model: tab.agent_session_model.clone(),
            chat_view: tab.chat_view,
            closed_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            workspace_id: String::new(),
            workspace_name: String::new(),
        }
    }
}

/// Legacy — kept only for old snapshot deserialization.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum Orientation {
    Horizontal,
    Vertical,
}

/// Tunable watcher limits set by the spawner; host clamps to safe bounds.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct WatcherPolicy {
    pub tick_interval_secs: u64,
    pub nudge_cooldown_secs: u64,
    pub max_actions_per_tick: u32,
}

impl WatcherPolicy {
    const DEFAULT_INTERVAL_SECS: u64 = 120;
    const MIN_INTERVAL_SECS: u64 = 30;
    const MAX_INTERVAL_SECS: u64 = 3600;
    const DEFAULT_COOLDOWN_SECS: u64 = 600;
    const MIN_COOLDOWN_SECS: u64 = 60;
    const DEFAULT_MAX_ACTIONS: u32 = 3;
    const MAX_MAX_ACTIONS: u32 = 5;

    pub fn clamped(interval: Option<u64>, cooldown: Option<u64>, max_actions: Option<u32>) -> Self {
        Self {
            tick_interval_secs: interval
                .unwrap_or(Self::DEFAULT_INTERVAL_SECS)
                .clamp(Self::MIN_INTERVAL_SECS, Self::MAX_INTERVAL_SECS),
            nudge_cooldown_secs: cooldown
                .unwrap_or(Self::DEFAULT_COOLDOWN_SECS)
                .max(Self::MIN_COOLDOWN_SECS),
            max_actions_per_tick: max_actions
                .unwrap_or(Self::DEFAULT_MAX_ACTIONS)
                .min(Self::MAX_MAX_ACTIONS),
        }
    }
}

/// Persisted watcher session state on a chatBackend tab.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct WatcherSpec {
    pub charter: String,
    pub policy: WatcherPolicy,
    #[serde(default)]
    pub observations: serde_json::Value,
    #[serde(default)]
    pub state: serde_json::Value,
    #[serde(default)]
    pub tick: u64,
    #[serde(default)]
    pub last_tick_ms: Option<u64>,
    #[serde(default)]
    pub last_status: Option<String>,
    #[serde(default)]
    pub tick_inflight_since_ms: Option<u64>,
}

/// What backs a tab's I/O. A `Pty` tab runs a terminal; a `ChatBackend` tab is
/// driven by a plugin's `chatBackend` capability and keeps `pty_id: None`
/// permanently (no backend reader, no screen state). The single discriminator the
/// `TabTransport` seam dispatches on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TabBacking {
    #[default]
    Pty,
    ChatBackend {
        plugin_id: String,
        session_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingDeliveryChannel {
    SystemPrompt,
    InteractiveInput,
    FallbackBootstrap,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingDeliveryProvenance {
    SystemPrompt,
    InteractiveInput,
    FallbackBootstrap,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryAttemptProvenance {
    pub attempt_id: String,
    pub started_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingDeliveryState {
    Pending,
    Confirmed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PendingDeliveryRecord {
    pub content_digest: String,
    pub channel: PendingDeliveryChannel,
    pub provenance: PendingDeliveryProvenance,
    state: PendingDeliveryState,
    attempt_provenance: Option<DeliveryAttemptProvenance>,
}

#[derive(Deserialize)]
struct PendingDeliveryRecordWire {
    content_digest: String,
    channel: PendingDeliveryChannel,
    #[serde(default)]
    provenance: Option<PendingDeliveryProvenance>,
    state: PendingDeliveryState,
    attempt_provenance: Option<DeliveryAttemptProvenance>,
}

impl<'de> Deserialize<'de> for PendingDeliveryRecord {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = PendingDeliveryRecordWire::deserialize(deserializer)?;
        let provenance = wire
            .provenance
            .unwrap_or_else(|| PendingDeliveryProvenance::for_channel(&wire.channel));
        Ok(Self {
            content_digest: wire.content_digest,
            channel: wire.channel,
            provenance,
            state: wire.state,
            attempt_provenance: wire.attempt_provenance,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingDeliveryTransitionError {
    ConfirmedTerminal,
    AttemptAlreadyInFlight,
}

impl PendingDeliveryRecord {
    pub fn new(content_digest: String, channel: PendingDeliveryChannel) -> Self {
        let provenance = PendingDeliveryProvenance::for_channel(&channel);
        Self {
            content_digest,
            channel,
            provenance,
            state: PendingDeliveryState::Pending,
            attempt_provenance: None,
        }
    }

    pub fn state(&self) -> PendingDeliveryState {
        self.state
    }

    pub fn attempt_provenance(&self) -> Option<&DeliveryAttemptProvenance> {
        self.attempt_provenance.as_ref()
    }

    pub fn begin_attempt(
        &mut self,
        provenance: DeliveryAttemptProvenance,
    ) -> Result<(), PendingDeliveryTransitionError> {
        match self.state {
            PendingDeliveryState::Confirmed => {
                Err(PendingDeliveryTransitionError::ConfirmedTerminal)
            }
            PendingDeliveryState::Pending if self.attempt_provenance.is_some() => {
                Err(PendingDeliveryTransitionError::AttemptAlreadyInFlight)
            }
            PendingDeliveryState::Pending => {
                self.attempt_provenance = Some(provenance);
                Ok(())
            }
        }
    }

    pub fn replace_attempt(
        &mut self,
        provenance: DeliveryAttemptProvenance,
    ) -> Result<(), PendingDeliveryTransitionError> {
        if self.state == PendingDeliveryState::Confirmed {
            return Err(PendingDeliveryTransitionError::ConfirmedTerminal);
        }
        self.attempt_provenance = Some(provenance);
        Ok(())
    }

    pub fn confirm(&mut self) -> Result<(), PendingDeliveryTransitionError> {
        if self.state == PendingDeliveryState::Confirmed {
            return Err(PendingDeliveryTransitionError::ConfirmedTerminal);
        }
        self.state = PendingDeliveryState::Confirmed;
        Ok(())
    }
}

impl PendingDeliveryProvenance {
    fn for_channel(channel: &PendingDeliveryChannel) -> Self {
        match channel {
            PendingDeliveryChannel::SystemPrompt => Self::SystemPrompt,
            PendingDeliveryChannel::InteractiveInput => Self::InteractiveInput,
            PendingDeliveryChannel::FallbackBootstrap => Self::FallbackBootstrap,
        }
    }
}

/// A tab represents a single terminal or browser panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tab {
    pub id: String,
    /// Sibling ordinal within a workspace or orchestration group.
    #[serde(default)]
    pub sort_order: i32,
    /// Linked PTY ID, if this tab has a running terminal.
    pub pty_id: Option<String>,
    /// What backs this tab's I/O (PTY by default, or a plugin chatBackend).
    #[serde(default)]
    pub backing: TabBacking,
    pub tab_type: TabType,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pending_delivery: Option<PendingDeliveryRecord>,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
    pub cwd: Option<String>,
    #[serde(skip, default)]
    pub provisioned_cwd: Option<String>,
    /// URL for browser tabs.
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
    /// When the output-based exit heuristic first suspected the agent left (no
    /// markers + shell prompt). Debounces a one-frame transient (e.g. a WebView
    /// reload/resize redraw) from clearing a still-running agent.
    #[serde(skip, default)]
    pub exit_suspected_at: Option<std::time::Instant>,
    /// True while launch_agent boots the agent TUI in this tab. A booting
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
    /// AgentProvider::extract_session_name. Searchable via `codeterm tab find`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub agent_session_name: Option<String>,
    /// Current model family on the live session ("Opus"/"Sonnet"/"Haiku").
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub agent_session_model: Option<String>,
    /// Model selected by plan/task routing before a launch-time override.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub agent_planned_model: Option<String>,
    /// Model explicitly requested at launch. Advisory only: the tab badge and
    /// reports use `agent_session_model`, which must come from runtime evidence.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub agent_requested_model: Option<String>,
    /// Model accepted by provider catalog resolution and passed to launch args.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub agent_provider_resolved_model: Option<String>,
    /// Structured catalog/runtime fallback provenance. Runtime actual is absent
    /// until provider-owned session evidence supplies it.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub model_fallback: Option<ModelFallbackNotice>,
    /// Runtime evidence that the provider ran a different model than requested.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub model_identity_mismatch: Option<ModelIdentityMismatch>,
    /// Per-launch binding nonce, injected as CODETERM_SESSION_BINDING_NONCE and echoed back by the provider's session-start report. Rejects stale/foreign reports claiming this tab.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub session_binding_nonce: Option<String>,
    /// Exact launch nonce echoed by the agent's accepted session-start report.
    /// This is cleared on every session transition; the configured nonce alone
    /// never proves that the current agent owns the tab.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reported_session_binding_nonce: Option<String>,
    /// Per-session nonce (env CODETERM_SHELL_NONCE) guarding OSC 633;E command text against forged output.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub shell_integration_nonce: Option<String>,
    /// Orchestration role (orchestrator / worker — 2-level, no middle tier).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub role: Option<AgentRole>,
    /// Behavioral profile layered on top of the structural role.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub role_profile: Option<String>,
    /// Team membership: the `(team_id, role, role_profile)` triple IS the
    /// membership relation. `None` = teamless. Minted at the spawn edge (Phase 2).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub team_id: Option<String>,
    /// Watcher-mode machine state (charter, policy, tick counters).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watcher: Option<WatcherSpec>,
    /// Parent tab ID in the orchestration hierarchy.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub parent_tab_id: Option<String>,
    /// Planner tab spawned as this Orchestrator's fresh pair before a plan
    /// exists. Consumed by the first plan create/goal path and never persisted.
    #[serde(skip, default)]
    pub pending_paired_planner_tab_id: Option<String>,
    /// Whether this tab can spawn child agents.
    #[serde(default)]
    pub can_spawn: bool,
    /// Depth in hierarchy: 0=none, 1=orchestrator, 2=worker (flat, 2-level).
    #[serde(default)]
    pub depth: u8,
    /// Report target tab ID — for cron agents that report without joining hierarchy.
    /// When `report_target_mesh_peer` is set, this is a tab id or `main`/`general`
    /// alias on that remote peer rather than a local tab.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub report_target_tab_id: Option<String>,
    /// Paired mesh peer (instance name or id) the report target lives on. Set by
    /// `agent spawn --mesh PEER` so a remote worker's `agent report` relays back
    /// to the spawning orchestrator / General target on another machine.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub report_target_mesh_peer: Option<String>,
    /// Tab id of the caller that ran `agent spawn` (CODETERM_TAB_ID at spawn time).
    /// Independent of `parent_tab_id` (which controls the role hierarchy and is
    /// forbidden for Orchestrators). Used as a final fallback by report routing
    /// so a top-level orchestrator's report still reaches its spawning agent.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub spawned_by_tab_id: Option<String>,
    /// Title naming priority level.
    #[serde(default)]
    pub title_level: TitleLevel,
    /// Whether chat view overlay is active for this tab.
    #[serde(default)]
    pub chat_view: bool,
    /// Pending interactive prompt (permission, confirmation, etc.)
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pending_prompt: Option<PendingPrompt>,
    /// Live 3-state agent classification (working / idle / question + subtype),
    /// computed from the PTY stream by the shared state engine and surfaced to
    /// the chat view. Persisted only with the tab-owned session identity.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub agent_state: Option<codeterm_types::pty::AgentLiveState>,
    /// Auto-approve prompts (set when spawned with skip_permissions).
    /// When true, detected prompts are answered automatically instead of shown to user.
    #[serde(default)]
    pub auto_approve: bool,
    /// Protected tabs cannot be closed via API or UI.
    #[serde(default)]
    pub protected: bool,
    /// Connected external services (e.g. ["telegram"]). Shown as icons next to tab title.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connected_services: Vec<String>,
    /// Hidden tabs are excluded from default tab listings and TabBar.
    /// Used by graph-worker tabs that live only in the memory panel.
    #[serde(default)]
    pub hidden: bool,
    /// Whether this tab should be rendered in the TabBar. Defaults to true.
    /// Graph-worker tabs set this to false even when temporarily focused.
    #[serde(default = "default_show_in_tabbar")]
    pub show_in_tabbar: bool,
    /// Graph-worker id linking this tab to a GraphWorkerRegistry entry.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub worker_id: Option<String>,
    /// In-memory only — true when tab was deserialized from a session
    /// snapshot. Gates the spawn-fresh-agent path in commands::pty so it
    /// does NOT race the orchestrator's launch on freshly spawned tabs.
    /// Cleared after the first successful spawn so a re-mount doesn't
    /// trigger another launch.
    #[serde(skip, default)]
    pub restored_from_snapshot: bool,
    /// In-memory only — when true, this tab is excluded from the session
    /// snapshot. Set for ephemeral picker/onboarding tabs that must not
    /// survive a daemon restart and reappear as ghost tabs.
    #[serde(skip, default)]
    pub skip_snapshot: bool,
    /// Orchestrator's opening greeting, prepended as a synthetic first
    /// assistant message every time `chat_start_core` builds this tab's
    /// transcript — never model-improvised. In-memory only (a restart
    /// simply drops it; it is not required to survive a daemon restart).
    #[serde(skip, default)]
    pub pending_greeting: Option<String>,
    /// Current agent-boot stage, surfaced to late UI attachers and persisted
    /// only with the tab-owned session identity.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub boot_stage: Option<codeterm_types::pty::BootStage>,
    /// Human-readable reason for a failed boot stage. Runtime-only and paired
    /// with `boot_stage == SpawnFailed`; cleared by any later non-failed stage.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub boot_failure_reason: Option<String>,
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

impl Tab {
    /// True when this tab is backed by a plugin `chatBackend` rather than a PTY.
    /// The one helper the I/O seam and frontend branch on instead of probing
    /// `pty_id`.
    pub fn is_chat_backend(&self) -> bool {
        matches!(self.backing, TabBacking::ChatBackend { .. })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum TabType {
    #[default]
    Terminal,
    /// Legacy — old snapshots only.
    Browser,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab_with(id: &str, backing: TabBacking) -> Tab {
        Tab {
            id: id.to_string(),
            sort_order: 0,
            pty_id: None,
            backing,
            tab_type: TabType::Terminal,
            pending_delivery: None,
            title: "Terminal".to_string(),
            description: None,
            cwd: None,
            provisioned_cwd: None,
            url: None,
            agent_type: None,
            agent_type_locked: false,
            agent_session_id: None,
            agent_session_name: None,
            agent_session_model: None,
            agent_planned_model: None,
            agent_requested_model: None,
            agent_provider_resolved_model: None,
            model_fallback: None,
            model_identity_mismatch: None,
            session_binding_nonce: None,
            reported_session_binding_nonce: None,
            shell_integration_nonce: None,
            role: None,
            role_profile: None,
            team_id: None,
            watcher: None,
            parent_tab_id: None,
            pending_paired_planner_tab_id: None,
            can_spawn: false,
            depth: 0,
            report_target_tab_id: None,
            report_target_mesh_peer: None,
            spawned_by_tab_id: None,
            title_level: TitleLevel::Default,
            chat_view: false,
            pending_prompt: None,
            agent_state: None,
            auto_approve: false,
            protected: false,
            connected_services: vec![],
            agent_set_at: None,
            exit_suspected_at: None,
            agent_booting: false,
            hidden: false,
            show_in_tabbar: true,
            worker_id: None,
            restored_from_snapshot: false,
            skip_snapshot: false,
            pending_greeting: None,
            boot_stage: None,
            boot_failure_reason: None,
        }
    }

    #[test]
    fn pty_tab_is_not_chat_backend() {
        let p = tab_with("a", TabBacking::Pty);
        assert!(!p.is_chat_backend());
    }

    #[test]
    fn confirmed_is_terminal() {
        let mut delivery =
            PendingDeliveryRecord::new("digest".into(), PendingDeliveryChannel::InteractiveInput);
        delivery
            .begin_attempt(DeliveryAttemptProvenance {
                attempt_id: "attempt-1".into(),
                started_at_ms: 42,
            })
            .unwrap();
        assert_eq!(delivery.confirm(), Ok(()));
        assert_eq!(delivery.state(), PendingDeliveryState::Confirmed);
        assert_eq!(
            delivery.begin_attempt(DeliveryAttemptProvenance {
                attempt_id: "attempt-2".into(),
                started_at_ms: 43,
            }),
            Err(PendingDeliveryTransitionError::ConfirmedTerminal)
        );
        assert_eq!(
            delivery.confirm(),
            Err(PendingDeliveryTransitionError::ConfirmedTerminal)
        );
    }

    #[test]
    fn legacy_pending_delivery_defaults_provenance_from_channel() {
        let record: PendingDeliveryRecord = serde_json::from_str(
            r#"{
                "content_digest":"legacy",
                "channel":"fallback_bootstrap",
                "state":"pending",
                "attempt_provenance":null
            }"#,
        )
        .unwrap();
        assert_eq!(record.channel, PendingDeliveryChannel::FallbackBootstrap);
        assert_eq!(
            record.provenance,
            PendingDeliveryProvenance::FallbackBootstrap
        );
    }

    #[test]
    fn pending_allows_one_inflight_attempt() {
        let mut delivery =
            PendingDeliveryRecord::new("digest".into(), PendingDeliveryChannel::InteractiveInput);
        delivery
            .begin_attempt(DeliveryAttemptProvenance {
                attempt_id: "attempt-1".into(),
                started_at_ms: 42,
            })
            .unwrap();
        assert_eq!(
            delivery.begin_attempt(DeliveryAttemptProvenance {
                attempt_id: "attempt-2".into(),
                started_at_ms: 43,
            }),
            Err(PendingDeliveryTransitionError::AttemptAlreadyInFlight)
        );
    }

    #[test]
    fn mid_boot_snapshot_exposes_current_stage() {
        use codeterm_types::pty::BootStage;
        // the live snapshot.
        let mut p = tab_with("a", TabBacking::Pty);
        p.boot_stage = Some(BootStage::AgentLaunching);
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["boot_stage"], "agent_launching");
        assert!(v.as_object().unwrap().get("boot_failure_reason").is_none());

        // Before any boot progress the field is omitted entirely (skip_if None).
        let idle = tab_with("b", TabBacking::Pty);
        let v = serde_json::to_value(&idle).unwrap();
        assert!(v.as_object().unwrap().get("boot_stage").is_none());
    }

    #[test]
    fn chat_backend_tab_is_detected() {
        let p = tab_with(
            "b",
            TabBacking::ChatBackend {
                plugin_id: "local-shell".into(),
                session_id: "s1".into(),
            },
        );
        assert!(p.is_chat_backend());
    }

    #[test]
    fn backing_round_trips_on_tab_and_defaults_to_pty() {
        // A ChatBackend tab survives a serde round-trip.
        let p = tab_with(
            "c",
            TabBacking::ChatBackend {
                plugin_id: "local-shell".into(),
                session_id: "s1".into(),
            },
        );
        let v = serde_json::to_value(&p).unwrap();
        let back: Tab = serde_json::from_value(v).unwrap();
        assert!(back.is_chat_backend());
        // snapshot) defaults to Pty.
        let mut v = serde_json::to_value(&p).unwrap();
        v.as_object_mut().unwrap().remove("backing");
        let back: Tab = serde_json::from_value(v).unwrap();
        assert_eq!(back.backing, TabBacking::Pty);
        assert!(!back.is_chat_backend());
    }

    #[test]
    fn chat_backing_round_trips_through_serde() {
        let backing = TabBacking::ChatBackend {
            plugin_id: "local-shell".into(),
            session_id: "abc".into(),
        };
        let v = serde_json::to_value(&backing).unwrap();
        assert_eq!(v["kind"], "chat_backend");
        let back: TabBacking = serde_json::from_value(v).unwrap();
        assert_eq!(back, backing);
    }

    #[test]
    fn watcher_policy_clamped_applies_defaults_and_bounds() {
        let all_none = WatcherPolicy::clamped(None, None, None);
        assert_eq!(all_none.tick_interval_secs, 120);
        assert_eq!(all_none.nudge_cooldown_secs, 600);
        assert_eq!(all_none.max_actions_per_tick, 3);

        let low = WatcherPolicy::clamped(Some(5), Some(0), Some(99));
        assert_eq!(low.tick_interval_secs, 30);
        assert_eq!(low.nudge_cooldown_secs, 60);
        assert_eq!(low.max_actions_per_tick, 5);
    }

    #[test]
    fn tab_with_watcher_round_trips_through_serde() {
        let spec = WatcherSpec {
            charter: "Observe the group.".into(),
            policy: WatcherPolicy::clamped(Some(90), Some(120), Some(2)),
            observations: serde_json::json!({"orchestration": true, "chatTail": 2}),
            state: serde_json::json!({"seen": 1}),
            tick: 3,
            last_tick_ms: Some(1_700_000_000_000),
            last_status: Some("ok".into()),
            tick_inflight_since_ms: None,
        };
        let mut p = tab_with("w1", TabBacking::Pty);
        p.watcher = Some(spec.clone());

        let v = serde_json::to_value(&p).unwrap();
        assert!(v.get("watcher").is_some());
        let back: Tab = serde_json::from_value(v).unwrap();
        assert_eq!(back.watcher, Some(spec));

        let mut bare = tab_with("w2", TabBacking::Pty);
        let v = serde_json::to_value(&bare).unwrap();
        assert!(v.get("watcher").is_none());
        bare.watcher = Some(WatcherSpec {
            charter: "x".into(),
            ..Default::default()
        });
        let back: Tab = serde_json::from_value(serde_json::to_value(&bare).unwrap()).unwrap();
        assert_eq!(back.watcher.as_ref().map(|w| w.charter.as_str()), Some("x"));
    }

    #[test]
    fn backing_matches_frontend_ts_contract_shape() {
        // The TS type is `{ kind: 'chat_backend'; plugin_id: string;
        let v = serde_json::to_value(TabBacking::ChatBackend {
            plugin_id: "lmstudio".into(),
            session_id: "s7".into(),
        })
        .unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "kind": "chat_backend",
                "plugin_id": "lmstudio",
                "session_id": "s7",
            })
        );
        // "not a chat Tab".
        assert_eq!(
            serde_json::to_value(TabBacking::Pty).unwrap(),
            serde_json::json!({ "kind": "pty" })
        );
    }
}
