use super::{ArchivedTab, ClosedStructure, ClosedStructureView, MultiplexerState, StructureState};

impl MultiplexerState {
    pub fn capture_structure(
        &self,
        workspace_id: &str,
        team_id: Option<&str>,
        id: String,
        now: u64,
    ) -> Result<ClosedStructure, String> {
        let workspace = self
            .get_workspace(workspace_id)
            .ok_or("Workspace not found")?
            .clone();
        if workspace.protected && team_id.is_none() {
            return Err("Cannot close protected workspace".into());
        }
        let teams: Vec<_> = self
            .list_teams()
            .into_iter()
            .filter(|team| {
                team.workspace_id == workspace_id && team_id.is_none_or(|id| team.id == id)
            })
            .collect();
        if team_id.is_some() && teams.is_empty() {
            return Err("Team not found".into());
        }
        let tabs = workspace
            .tabs
            .iter()
            .filter_map(|id| self.get_tab(id))
            .filter(|tab| team_id.is_none_or(|id| tab.team_id.as_deref() == Some(id)))
            .map(|tab| ArchivedTab {
                tab: tab.clone(),
                launch: tab.restorable_launch.clone(),
            })
            .collect();
        let label = team_id
            .and_then(|id| teams.iter().find(|team| team.id == id))
            .map(|team| team.name.clone().unwrap_or_else(|| team.id.clone()))
            .unwrap_or_else(|| workspace.name.clone());
        Ok(ClosedStructure {
            id,
            label,
            kind: if team_id.is_some() {
                "team"
            } else {
                "workspace"
            }
            .into(),
            closed_at: now,
            undo_until: now.saturating_add(600_000),
            state: StructureState::Captured,
            workspace,
            teams,
            tabs,
            resources: Default::default(),
        })
    }

    pub fn save_closed_structure(&self, record: &ClosedStructure) -> Result<(), String> {
        self.closed_store.save_structure(record)
    }

    pub fn closed_structure(&self, id: &str) -> Result<Option<ClosedStructure>, String> {
        self.closed_store.structure(id)
    }

    pub fn closed_structure_records(&self) -> Result<Vec<ClosedStructure>, String> {
        self.closed_store.structures()
    }

    pub fn finish_structure_close(&self, id: &str) -> Result<(), String> {
        self.closed_store.finish_structure_close(id)
    }

    pub fn discard_closed_structure(&self, id: &str) -> Result<(), String> {
        self.closed_store.discard_structure(id)
    }

    pub(super) fn closed_structure_views(&self) -> Vec<ClosedStructureView> {
        match self.closed_store.structures() {
            Ok(records) => records
                .iter()
                .filter(|record| record.state != StructureState::Captured)
                .map(ClosedStructure::view)
                .collect(),
            Err(error) => {
                log::error!("[closed-history] inventory read failed: {error}");
                Vec::new()
            }
        }
    }

    pub fn restore_structure(
        &mut self,
        record: &ClosedStructure,
    ) -> Result<(String, String), String> {
        let tabs = record.restore_tabs()?;
        for tab in &tabs {
            if let Some(existing) = self.get_tab(&tab.id) {
                if existing
                    .close_restore
                    .as_ref()
                    .and_then(|state| state.record_id.as_deref())
                    != Some(record.id.as_str())
                {
                    if existing.agent_type != tab.agent_type
                        || existing.agent_session_id != tab.agent_session_id
                        || existing.team_id != tab.team_id
                        || existing.cwd != tab.cwd
                    {
                        return Err(format!("Tab identity {} is already in use", tab.id));
                    }
                }
            }
        }
        if let Some(existing) = self.get_workspace(&record.workspace.id) {
            if existing.cwd != record.workspace.cwd {
                return Err("Workspace identity is already in use at another cwd".into());
            }
        }
        for team in &record.teams {
            if self
                .get_team(&team.id)
                .is_some_and(|existing| existing.workspace_id != team.workspace_id)
            {
                return Err(format!("Team identity {} is already in use", team.id));
            }
        }
        let mut transaction = self.begin_team_close_transaction();
        let staged = transaction.state_mut();
        if staged.get_workspace(&record.workspace.id).is_none() {
            let mut workspace = record.workspace.clone();
            workspace.tabs.clear();
            workspace.active_tab_id = None;
            staged.workspaces.push(workspace);
        }
        for team in &record.teams {
            staged.upsert_team(team.clone());
        }
        for tab in tabs {
            if !staged.tabs.contains_key(&tab.id) {
                staged.tabs.insert(tab.id.clone(), tab);
            }
        }
        let workspace = staged
            .workspaces
            .iter_mut()
            .find(|workspace| workspace.id == record.workspace.id)
            .ok_or("Restored workspace missing")?;
        let mut order: Vec<_> = record
            .workspace
            .tabs
            .iter()
            .filter(|id| staged.tabs.contains_key(*id))
            .cloned()
            .collect();
        for id in &workspace.tabs {
            if !order.contains(id) {
                order.push(id.clone());
            }
        }
        workspace.tabs = order;
        workspace.active_tab_id = record
            .workspace
            .active_tab_id
            .clone()
            .filter(|id| workspace.tabs.contains(id))
            .or_else(|| workspace.tabs.first().cloned());
        let tab_id = workspace.active_tab_id.clone().unwrap_or_default();
        staged.active_workspace_id = Some(workspace.id.clone());
        staged.focused_tab_id = (!tab_id.is_empty()).then(|| tab_id.clone());
        transaction.commit(self, &[]);
        Ok((record.workspace.id.clone(), tab_id))
    }
}
