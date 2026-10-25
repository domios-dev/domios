use std::collections::{BTreeMap, BTreeSet};

use codeterm_service_core::multiplexer::{state::MultiplexerState, types::Tab};
use codeterm_types::team::Team;

pub trait MuxView {
    fn has_workspace(&self, workspace: &str) -> bool;
    fn get_tab(&self, id: &str) -> Option<&Tab>;
    fn find_workspace_for_tab(&self, id: &str) -> Option<String>;
    fn get_team(&self, id: &str) -> Option<&Team>;
    fn team_workspace(&self, id: &str) -> Option<String>;
}

impl MuxView for MultiplexerState {
    fn has_workspace(&self, workspace: &str) -> bool {
        self.get_workspace(workspace).is_some()
    }
    fn get_tab(&self, id: &str) -> Option<&Tab> {
        self.get_tab(id)
    }
    fn find_workspace_for_tab(&self, id: &str) -> Option<String> {
        self.find_workspace_for_tab(id)
    }
    fn get_team(&self, id: &str) -> Option<&Team> {
        self.get_team(id)
    }
    fn team_workspace(&self, id: &str) -> Option<String> {
        self.team_workspace(id)
    }
}

pub(crate) struct MuxSnapshot {
    workspace: String,
    tabs: BTreeMap<String, Tab>,
    teams: BTreeMap<String, (Team, Option<String>)>,
}

impl MuxSnapshot {
    pub(crate) fn capture(
        mux: &MultiplexerState,
        workspace: &str,
        tab_ids: &BTreeSet<String>,
        team_ids: &BTreeSet<String>,
    ) -> Result<Self, String> {
        super::validate_scope(mux, workspace, None)?;
        let tabs: BTreeMap<_, _> = tab_ids
            .iter()
            .filter(|id| mux.find_workspace_for_tab(id).as_deref() == Some(workspace))
            .filter_map(|id| mux.get_tab(id).map(|tab| (id.clone(), tab.clone())))
            .collect();
        let teams = team_ids
            .iter()
            .filter_map(|id| mux.get_team(id).cloned())
            .map(|team| {
                let current_workspace = mux.team_workspace(&team.id);
                (team.id.clone(), (team, current_workspace))
            })
            .collect();
        Ok(Self {
            workspace: workspace.into(),
            tabs,
            teams,
        })
    }
}

impl MuxView for MuxSnapshot {
    fn has_workspace(&self, workspace: &str) -> bool {
        self.workspace == workspace
    }
    fn get_tab(&self, id: &str) -> Option<&Tab> {
        self.tabs.get(id)
    }
    fn find_workspace_for_tab(&self, id: &str) -> Option<String> {
        self.tabs.contains_key(id).then(|| self.workspace.clone())
    }
    fn get_team(&self, id: &str) -> Option<&Team> {
        self.teams.get(id).map(|(team, _)| team)
    }
    fn team_workspace(&self, id: &str) -> Option<String> {
        self.teams
            .get(id)
            .and_then(|(_, workspace)| workspace.clone())
    }
}
