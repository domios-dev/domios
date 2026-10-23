use super::MultiplexerState;
use codeterm_entitlements::{LimitGate, LimitKey, LockCandidate, LockSet, LockedRefusal};

/// Plan-limit lock marks per item kind; marking never removes an item from a listing.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct MuxLocks {
    pub workspaces: LockSet,
    pub teams: LockSet,
    #[serde(rename = "agent.tabs")]
    pub agent_tabs: LockSet,
}

impl MuxLocks {
    pub fn is_locked(&self, limit: LimitKey, id: &str) -> bool {
        match limit {
            LimitKey::Workspaces => self.workspaces.contains(id),
            LimitKey::Teams => self.teams.contains(id),
            LimitKey::AgentTabs => self.agent_tabs.contains(id),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LimitGateChange {
    pub gate: bool,
    pub locks: bool,
}

impl MultiplexerState {
    pub fn set_limit_gate(&mut self, gate: LimitGate) -> LimitGateChange {
        if self.limit_gate == gate {
            return LimitGateChange::default();
        }
        let before = self.locks();
        self.limit_gate = gate;
        LimitGateChange {
            gate: true,
            locks: before != self.locks(),
        }
    }

    pub fn locks(&self) -> MuxLocks {
        let gate = &self.limit_gate;
        let workspaces: Vec<LockCandidate> = self
            .workspaces
            .iter()
            .map(|workspace| LockCandidate {
                id: workspace.id.clone(),
                created_at: workspace.created_at_ms,
                exempt: workspace.protected,
            })
            .collect();
        let teams: Vec<LockCandidate> = self
            .teams
            .values()
            .map(|team| LockCandidate {
                id: team.id.clone(),
                created_at: team.created_at,
                exempt: false,
            })
            .collect();
        let agent_tabs: Vec<LockCandidate> = self
            .tabs
            .values()
            .map(|tab| LockCandidate {
                id: tab.id.clone(),
                created_at: tab.created_at_ms,
                exempt: false,
            })
            .collect();
        MuxLocks {
            workspaces: gate.over_limit(LimitKey::Workspaces, &workspaces),
            teams: gate.over_limit(LimitKey::Teams, &teams),
            agent_tabs: gate.over_limit(LimitKey::AgentTabs, &agent_tabs),
        }
    }

    /// A write into `tab_id` is refused when the tab, its workspace, or its Team is locked.
    pub fn tab_write_lock(&self, tab_id: &str) -> Option<LockedRefusal> {
        let tab_id = self.resolve_tab_id(tab_id)?;
        let team = self.get_tab(&tab_id).and_then(|tab| tab.team_id.clone());
        let workspace = self.find_workspace_for_tab(&tab_id);
        self.write_lock(
            [
                (LimitKey::AgentTabs, Some(tab_id)),
                (LimitKey::Workspaces, workspace),
                (LimitKey::Teams, team),
            ]
            .into_iter()
            .filter_map(|(limit, id)| Some((limit, id?))),
        )
    }

    /// Title and description are writes too: a locked tab keeps its last metadata.
    pub(crate) fn refuse_locked_tab(&self, tab_id: &str) -> Result<(), String> {
        self.tab_write_lock(tab_id)
            .map_or(Ok(()), |refusal| Err(refusal.to_string()))
    }

    /// A write into a workspace or Team (spawn, plan/team mutation) is refused when it is locked.
    pub fn item_write_lock(&self, limit: LimitKey, id: &str) -> Option<LockedRefusal> {
        self.write_lock(std::iter::once((limit, id.to_string())))
    }

    fn write_lock(
        &self,
        items: impl IntoIterator<Item = (LimitKey, String)>,
    ) -> Option<LockedRefusal> {
        if !self.limit_gate.summary().enforcing {
            return None;
        }
        let locks = self.locks();
        items
            .into_iter()
            .find(|(limit, id)| locks.is_locked(*limit, id))
            .map(|(limit, item_id)| LockedRefusal { limit, item_id })
    }
}

#[cfg(test)]
mod tests;
