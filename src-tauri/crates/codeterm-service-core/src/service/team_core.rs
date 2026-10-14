use crate::multiplexer::state::MultiplexerState;
use crate::service::plan::PlanError;

#[derive(Debug)]
pub(crate) enum TeamPlanError {
    Team(String),
    Plan(PlanError),
}

/// Who is asking to mutate a Team. Headerless REST callers need a confirmed
/// target echo; Tauri and local socket callers carry their own local identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TeamMutationCaller {
    AgentTab(String),
    AuthenticatedMeshPeer(String),
    Human,
    HumanConfirmedHeaderless,
    Unattributed,
}

impl TeamMutationCaller {
    pub(crate) fn from_tab(caller_tab_id: Option<&str>) -> Self {
        match caller_tab_id.map(str::trim).filter(|id| !id.is_empty()) {
            Some(tab_id) => Self::AgentTab(tab_id.to_string()),
            None => Self::Human,
        }
    }

    pub(super) fn tab_id(&self) -> Option<&str> {
        match self {
            Self::AgentTab(tab_id) => Some(tab_id),
            Self::AuthenticatedMeshPeer(_)
            | Self::Human
            | Self::HumanConfirmedHeaderless
            | Self::Unattributed => None,
        }
    }

    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::AgentTab(_) => "AgentTab",
            Self::AuthenticatedMeshPeer(_) => "AuthenticatedMeshPeer",
            Self::Human => "Human",
            Self::HumanConfirmedHeaderless => "human-attributed (headerless, confirmed)",
            Self::Unattributed => "Unattributed",
        }
    }
}

pub(super) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Local tabs that anchor this Team. A mirrored (remote) Team has members here
/// but no anchor, which is what makes it locally immutable.
pub(super) fn local_team_anchors(mux: &MultiplexerState, team_id: &str) -> Vec<String> {
    let mut anchors: Vec<String> = mux
        .snapshot()
        .tabs
        .values()
        .filter(|tab| tab.team_id.as_deref() == Some(team_id) && tab.anchors_team())
        .map(|tab| tab.id.clone())
        .collect();
    anchors.sort();
    anchors
}

/// The peer a Team is anchored on, read from existing `RemoteOwnership`
/// evidence. `RemoteOwnership` itself is untouched: this only reads it.
pub(super) fn remote_team_origin(mux: &MultiplexerState, team_id: &str) -> Option<String> {
    mux.snapshot().tabs.values().find_map(|tab| {
        let ownership = tab.ownership.as_ref()?;
        let team = ownership.team.as_ref()?;
        let qualified_id = format!("mesh:{}/{}", ownership.peer_id, team.id);
        (team.id == team_id || qualified_id == team_id).then(|| ownership.peer_name.clone())
    })
}

pub(super) fn is_remote_team(mux: &MultiplexerState, team_id: &str) -> bool {
    local_team_anchors(mux, team_id).is_empty() && remote_team_origin(mux, team_id).is_some()
}
