use super::MultiplexerState;
use codeterm_types::plan::task_key::WorkspaceTaskKeys;
use std::collections::{HashMap, HashSet};

impl MultiplexerState {
    pub fn workspace_task_keys(&self, workspace_id: &str) -> Option<&WorkspaceTaskKeys> {
        self.get_workspace(workspace_id).map(|ws| &ws.task_keys)
    }

    /// The workspace whose key state a plan draws from: its Team's live home,
    /// else the workspace opened on the plan's project folder.
    pub fn task_key_workspace(
        &self,
        team_id: Option<&str>,
        project_paths: &[&str],
    ) -> Option<String> {
        if let Some(team_id) = team_id {
            let team_home = self.team_workspace(team_id).or_else(|| {
                self.get_team(team_id)
                    .map(|team| team.workspace_id.clone())
                    .filter(|id| self.get_workspace(id).is_some())
            });
            if team_home.is_some() {
                return team_home;
            }
        }
        project_paths
            .iter()
            .filter(|path| !path.trim().is_empty())
            .find_map(|path| self.find_workspace_by_cwd(path).map(|ws| ws.id.clone()))
    }

    /// Issues keys under the one mux write lock, so prefix choice and counter
    /// advance are atomic against every other local workspace.
    pub fn issue_task_keys(
        &mut self,
        workspace_id: &str,
        count: usize,
        high_water: &HashMap<String, u64>,
    ) -> Vec<String> {
        let taken: HashSet<String> = self
            .workspaces
            .iter()
            .filter(|ws| ws.id != workspace_id)
            .filter_map(|ws| ws.task_keys.prefix.clone())
            .collect();
        let Some(ws) = self.workspaces.iter_mut().find(|ws| ws.id == workspace_id) else {
            return Vec::new();
        };
        let name = ws.name.clone();
        (0..count)
            .map(|_| ws.task_keys.issue(&name, &taken, high_water))
            .collect()
    }
}
