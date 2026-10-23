use super::{LimitGate, LimitKey};
use crate::multiplexer::state::MultiplexerState;
use codeterm_entitlements::{LockCandidate, LockSet};
use std::collections::HashMap;

/// Lock marks for every limited item kind; listings keep every item regardless.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct LockProjection {
    pub workspaces: LockSet,
    pub teams: LockSet,
    #[serde(rename = "agent.tabs")]
    pub agent_tabs: LockSet,
}

impl LockProjection {
    pub fn project(gate: &LimitGate, items: &LockItems) -> Self {
        Self {
            workspaces: gate.over_limit(LimitKey::Workspaces, &items.workspaces),
            teams: gate.over_limit(LimitKey::Teams, &items.teams),
            agent_tabs: gate.over_limit(LimitKey::AgentTabs, &items.agent_tabs),
        }
    }

    pub fn is_locked(&self, limit: LimitKey, id: &str) -> bool {
        match limit {
            LimitKey::Workspaces => self.workspaces.contains(id),
            LimitKey::Teams => self.teams.contains(id),
            LimitKey::AgentTabs => self.agent_tabs.contains(id),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct LockItems {
    pub workspaces: Vec<LockCandidate>,
    pub teams: Vec<LockCandidate>,
    pub agent_tabs: Vec<LockCandidate>,
}

impl LockItems {
    /// Workspaces and tabs carry no creation stamp, so their mux order stands in as the ordinal.
    pub fn from_mux(mux: &MultiplexerState) -> Self {
        let workspaces = mux.list_workspaces();
        let tab_order: HashMap<&str, u64> = workspaces
            .iter()
            .flat_map(|ws| ws.tab_ids.iter())
            .enumerate()
            .map(|(index, id)| (id.as_str(), index as u64))
            .collect();
        let agent_tabs = mux
            .all_tabs()
            .map(|tab| LockCandidate {
                id: tab.id.clone(),
                created_at: tab_order.get(tab.id.as_str()).copied().unwrap_or(u64::MAX),
                exempt: false,
            })
            .collect();
        Self {
            workspaces: workspaces
                .iter()
                .enumerate()
                .map(|(index, ws)| LockCandidate {
                    id: ws.id.clone(),
                    created_at: index as u64,
                    exempt: ws.protected,
                })
                .collect(),
            teams: mux
                .list_teams()
                .into_iter()
                .map(|team| LockCandidate {
                    id: team.id,
                    created_at: team.created_at,
                    exempt: false,
                })
                .collect(),
            agent_tabs,
        }
    }
}

#[cfg(test)]
mod tests;
