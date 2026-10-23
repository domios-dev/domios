use super::MultiplexerState;
use codeterm_entitlements::{LimitGate, LimitKey, LockCandidate, LockSet};

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
}

#[cfg(test)]
mod tests;
