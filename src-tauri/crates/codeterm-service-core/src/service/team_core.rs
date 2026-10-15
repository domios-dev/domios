use crate::multiplexer::state::MultiplexerState;
use crate::service::plan::PlanError;
use crate::service::ServiceContext;

#[derive(Debug)]
pub enum TeamPlanError {
    Team(String),
    Plan(PlanError),
}

/// Who is asking to mutate a Team. Headerless REST callers need a confirmed
/// target echo; Tauri and local socket callers carry their own local identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeamMutationCaller {
    AgentTab(String),
    AuthenticatedMeshPeer(String),
    Human,
    HumanConfirmedHeaderless,
    Unattributed,
}

impl TeamMutationCaller {
    pub fn from_tab(caller_tab_id: Option<&str>) -> Self {
        match caller_tab_id.map(str::trim).filter(|id| !id.is_empty()) {
            Some(tab_id) => Self::AgentTab(tab_id.to_string()),
            None => Self::Human,
        }
    }

    pub fn tab_id(&self) -> Option<&str> {
        match self {
            Self::AgentTab(tab_id) => Some(tab_id),
            Self::AuthenticatedMeshPeer(_)
            | Self::Human
            | Self::HumanConfirmedHeaderless
            | Self::Unattributed => None,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::AgentTab(_) => "AgentTab",
            Self::AuthenticatedMeshPeer(_) => "AuthenticatedMeshPeer",
            Self::Human => "Human",
            Self::HumanConfirmedHeaderless => "human-attributed (headerless, confirmed)",
            Self::Unattributed => "Unattributed",
        }
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Local tabs that anchor this Team. A mirrored (remote) Team has members here
/// but no anchor, which is what makes it locally immutable.
pub fn local_team_anchors(mux: &MultiplexerState, team_id: &str) -> Vec<String> {
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
pub fn remote_team_origin(mux: &MultiplexerState, team_id: &str) -> Option<String> {
    mux.snapshot().tabs.values().find_map(|tab| {
        let ownership = tab.ownership.as_ref()?;
        let team = ownership.team.as_ref()?;
        let qualified_id = format!("mesh:{}/{}", ownership.peer_id, team.id);
        (team.id == team_id || qualified_id == team_id).then(|| ownership.peer_name.clone())
    })
}

pub fn is_remote_team(mux: &MultiplexerState, team_id: &str) -> bool {
    local_team_anchors(mux, team_id).is_empty() && remote_team_origin(mux, team_id).is_some()
}

pub fn live_team_ids(svc: &ServiceContext) -> std::collections::BTreeSet<String> {
    svc.mux
        .read()
        .list_teams()
        .into_iter()
        .map(|team| team.id)
        .collect()
}

pub fn reconcile_dangling_team_plan_bindings_and_emit(
    svc: &ServiceContext,
    live_team_ids: &std::collections::BTreeSet<String>,
    now_ms: u64,
) -> Result<Vec<String>, crate::service::plan::PlanError> {
    let changed_plan_ids = svc
        .plan_store
        .archive_active_plans_without_live_team(live_team_ids, now_ms)?;
    for plan_id in &changed_plan_ids {
        svc.emit_plan_changed(plan_id);
    }
    Ok(changed_plan_ids)
}

pub fn valid_live_plan_team(
    svc: &ServiceContext,
    mux: &MultiplexerState,
    plan_id: &str,
) -> Result<String, TeamPlanError> {
    let plan = svc
        .plan_store
        .get(plan_id)
        .ok_or(TeamPlanError::Plan(PlanError::NotFound))?;
    let team_id = plan
        .team_id
        .filter(|team_id| !team_id.trim().is_empty())
        .ok_or_else(|| {
            TeamPlanError::Plan(PlanError::ActivePlanRequiresTeam {
                plan_id: plan_id.into(),
            })
        })?;
    if mux.get_team(&team_id).is_none() {
        return Err(TeamPlanError::Team(format!(
            "Plan {plan_id} references missing Team {team_id}; adopt it into a live Team before resuming"
        )));
    }
    Ok(team_id)
}
