use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use uuid::Uuid;

use super::types::{AgentRole, Tab, TabBacking, TabType, TitleLevel, Workspace};
use codeterm_types::agent::role_kind;
use codeterm_types::team::Team;

mod alias;
mod closed;
pub mod names;
mod reorder;
mod role;
mod snapshot;
mod tab;
mod tab_runtime;
mod workspace;

pub use closed::{
    ClosedGroup, ClosedTabView, ClosedTabsStore, GroupKind, PreparedWorkspaceClose,
    TabCloseReservation, CLOSED_GROUP_CAP, RECENTLY_CLOSED_CAP, RECENTLY_CLOSED_VIEW_LIMIT,
};
pub use snapshot::SnapshotMergeReport;

#[cfg(test)]
mod tests;

/// Generate a short 8-char hex ID (from UUID v4).
fn short_id() -> String {
    let uuid = Uuid::new_v4();
    let bytes = uuid.as_bytes();
    format!(
        "{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3]
    )
}

/// Lightweight workspace summary for listing.
#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceInfo {
    pub id: String,
    pub name: String,
    pub tab_count: usize,
    pub is_active: bool,
    pub cwd: Option<String>,
    pub protected: bool,
    pub tab_ids: Vec<String>,
    /// Set to true when create_workspace deduped by cwd (no new ws was created).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing: Option<bool>,
}

/// Central multiplexer state: workspaces, tabs, tabs, and their relationships.
pub struct MultiplexerState {
    workspaces: Vec<Workspace>,
    tabs: HashMap<String, Tab>,
    active_workspace_id: Option<String>,
    focused_tab_id: Option<String>,
    /// Reverse lookup: pty_id -> tab_id for O(1) resolution.
    pty_to_tab: HashMap<String, String>,
    /// Alias registry: human-readable name -> tab_id.
    tab_aliases: HashMap<String, String>,
    /// Team registry: team_id -> Team. Membership itself lives on each tab's
    /// `team_id`; this holds the team's own metadata (name, lifecycle). Persisted
    /// in the session snapshot.
    teams: HashMap<String, Team>,
    /// Monotonic snapshot generation. Every `snapshot()` stamps a strictly
    /// increasing value so the frontend reducer can ignore a stale snapshot
    /// (older generation) that resolves after a newer one — kills the
    /// spawn-then-blink race where a pre-tab `mux_get_state` clobbers the tab.
    generation: AtomicU64,
    /// Durable browser-style closed-tab history. Leaf lock (its own
    /// `Mutex<Connection>`); only ever touched while the mux lock is held, never
    /// across an await. Defaults to in-memory; production swaps in a file-backed
    /// store at startup via [`MultiplexerState::attach_closed_store`].
    pub(super) closed_store: ClosedTabsStore,
    workspace_close_reservations: HashMap<String, String>,
}

pub struct TeamCloseTransaction {
    staged: MultiplexerState,
}

impl TeamCloseTransaction {
    pub fn state_mut(&mut self) -> &mut MultiplexerState {
        &mut self.staged
    }

    pub fn commit(self, target: &mut MultiplexerState, closed_roots: &[String]) {
        for tab_id in closed_roots {
            target.record_cascade_close(tab_id);
        }
        target.workspaces = self.staged.workspaces;
        target.tabs = self.staged.tabs;
        target.active_workspace_id = self.staged.active_workspace_id;
        target.focused_tab_id = self.staged.focused_tab_id;
        target.pty_to_tab = self.staged.pty_to_tab;
        target.tab_aliases = self.staged.tab_aliases;
        target.teams = self.staged.teams;
        target.generation.store(
            self.staged.generation.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamResolution {
    pub team_id: String,
    pub minted: bool,
}

/// Full state snapshot for frontend hydration.
#[derive(Debug, Clone, Serialize)]
pub struct MultiplexerSnapshot {
    pub workspaces: Vec<Workspace>,
    pub tabs: HashMap<String, Tab>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub teams: Vec<Team>,
    pub active_workspace_id: Option<String>,
    pub focused_tab_id: Option<String>,
    /// Monotonic generation; the reducer drops any snapshot whose value is
    /// `<=` the one already applied.
    pub generation: u64,
    /// Bounded newest-first view of the closed-tab history (across all
    /// workspaces) for the sidebar. Sourced live from the sqlite store, NOT the
    /// persisted session snapshot — keeps both lean.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recently_closed: Vec<ClosedTabView>,
    /// Aggregate Idle/Working/Waiting/Dead per PTY-backed tab, mirrored from
    /// `TabStatusTracker` so a fresh page load hydrates the same rail color a
    /// live `tab-status-changed` event would have delivered. Only populated by
    /// [`MultiplexerState::snapshot_with_status`] — plain `snapshot()` leaves it
    /// empty since most internal callers don't need it.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub tab_statuses: HashMap<String, codeterm_types::pty::TabStatus>,
}

impl MultiplexerState {
    pub fn begin_team_close_transaction(&self) -> TeamCloseTransaction {
        TeamCloseTransaction {
            staged: Self {
                workspaces: self.workspaces.clone(),
                tabs: self.tabs.clone(),
                active_workspace_id: self.active_workspace_id.clone(),
                focused_tab_id: self.focused_tab_id.clone(),
                pty_to_tab: self.pty_to_tab.clone(),
                tab_aliases: self.tab_aliases.clone(),
                teams: self.teams.clone(),
                generation: AtomicU64::new(self.generation.load(Ordering::Relaxed)),
                closed_store: ClosedTabsStore::in_memory(),
                workspace_close_reservations: self.workspace_close_reservations.clone(),
            },
        }
    }
    /// Create an empty state with no workspaces.
    /// Used when General Agent will create its own workspace.
    pub fn empty() -> Self {
        Self {
            workspaces: vec![],
            tabs: HashMap::new(),
            active_workspace_id: None,
            focused_tab_id: None,
            pty_to_tab: HashMap::new(),
            tab_aliases: HashMap::new(),
            teams: HashMap::new(),
            generation: AtomicU64::new(0),
            closed_store: ClosedTabsStore::in_memory(),
            workspace_close_reservations: HashMap::new(),
        }
    }

    /// Register or replace a team by its id.
    pub fn upsert_team(&mut self, team: Team) {
        self.teams.insert(team.id.clone(), team);
    }

    /// Look up a team by id.
    pub fn get_team(&self, id: &str) -> Option<&Team> {
        self.teams.get(id)
    }

    /// Where a Team currently lives, derived from its member tabs and preferring
    /// its anchor. `Team.workspace_id` is mint-time provenance, so it never
    /// follows a tab moved to another workspace.
    pub fn team_workspace(&self, team_id: &str) -> Option<String> {
        self.teams.get(team_id)?;
        let mut members: Vec<(bool, &str)> = self
            .tabs
            .values()
            .filter(|tab| tab.team_id.as_deref() == Some(team_id))
            .map(|tab| (!tab.anchors_team(), tab.id.as_str()))
            .collect();
        members.sort_unstable();
        members
            .into_iter()
            .find_map(|(_, tab_id)| self.find_workspace_for_tab(tab_id))
    }

    /// Remove a team by id, returning it if present.
    pub fn remove_team(&mut self, id: &str) -> Option<Team> {
        self.teams.remove(id)
    }

    /// All registered teams (unordered).
    pub fn list_teams(&self) -> Vec<Team> {
        let mut teams: Vec<_> = self.teams.values().cloned().collect();
        teams.sort_by(|a, b| a.id.cmp(&b.id));
        teams
    }

    pub fn rename_team(&mut self, id: &str, name: String) -> Result<Team, String> {
        let team = self
            .teams
            .get_mut(id)
            .ok_or_else(|| format!("Team {id} not found"))?;
        team.name = Some(name);
        Ok(team.clone())
    }

    /// Number of registered teams.
    pub fn team_count(&self) -> usize {
        self.teams.len()
    }

    /// Swap the in-memory closed-history store for a durable file-backed one.
    /// Called once at startup so closed-tab history survives daemon restarts.
    pub fn attach_closed_store(&mut self, store: ClosedTabsStore) {
        self.closed_store = store;
    }

    /// Create a new state with a default "Main" workspace and one tab.
    /// Returns (state, initial_tab_id).
    pub fn new() -> (Self, String) {
        let tab_id = short_id();
        let workspace_id = short_id();

        let tab = Tab {
            id: tab_id.clone(),
            sort_order: 0,
            pty_id: None,
            backing: TabBacking::default(),
            tab_type: TabType::Terminal,
            pending_delivery: None,
            title: names::generate_tab_name(),
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
            reasoning_effort_identity: Default::default(),
            context_epoch_archives: vec![],
            session_binding_nonce: None,
            reported_session_binding_nonce: None,
            prior_session_binding_nonces: Vec::new(),
            shell_integration_nonce: None,
            lifecycle_epoch: None,
            lifecycle_operation_id: None,
            task_binding: None,
            roleless_tracked: false,
            role: Some(AgentRole::Manager),
            role_profile: Some(role_kind::builtins::ORCHESTRATOR.to_string()),
            team_id: None,
            watcher: None,
            parent_tab_id: None,
            pending_paired_planner_tab_id: None,
            can_spawn: true,
            depth: 1,
            report_target_tab_id: None,
            report_target_mesh_peer: None,
            spawned_by_tab_id: None,
            ownership: None,
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
            audience: Default::default(),
            skip_snapshot: false,
            show_in_tabbar: true,
            worker_id: None,
            restored_from_snapshot: false,
            pending_greeting: None,
            boot_stage: None,
            boot_failure_reason: None,
        };

        let workspace = Workspace {
            id: workspace_id.clone(),
            name: "Main".to_string(),
            tabs: vec![tab_id.clone()],
            active_tab_id: Some(tab_id.clone()),
            cwd: None,
            protected: false,
            sort_order: 0,
        };

        let mut tabs = HashMap::new();
        tabs.insert(tab_id.clone(), tab);

        let state = Self {
            workspaces: vec![workspace],
            tabs,
            active_workspace_id: Some(workspace_id),
            focused_tab_id: Some(tab_id.clone()),
            pty_to_tab: HashMap::new(),
            tab_aliases: HashMap::new(),
            teams: HashMap::new(),
            generation: AtomicU64::new(0),
            closed_store: ClosedTabsStore::in_memory(),
            workspace_close_reservations: HashMap::new(),
        };

        (state, tab_id)
    }
}
