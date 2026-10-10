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
                team_id.map_or_else(
                    || {
                        team.workspace_id == workspace_id
                            || workspace.tabs.iter().any(|id| {
                                self.get_tab(id).is_some_and(|tab| {
                                    tab.team_id.as_deref() == Some(team.id.as_str())
                                })
                            })
                    },
                    |id| team.id == id,
                )
            })
            .collect();
        if team_id.is_some() && teams.is_empty() {
            return Err("Team not found".into());
        }
        let team_ids: std::collections::HashSet<_> =
            teams.iter().map(|team| team.id.as_str()).collect();
        let tabs: Vec<_> = self
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.tabs.iter())
            .filter_map(|id| self.get_tab(id))
            .filter(|tab| {
                tab.team_id
                    .as_deref()
                    .is_some_and(|id| team_ids.contains(id))
                    || (team_id.is_none() && workspace.tabs.contains(&tab.id))
            })
            .map(|tab| ArchivedTab {
                tab: tab.clone(),
                launch: tab.restorable_launch.clone(),
                paired_planner_tab_id: tab.pending_paired_planner_tab_id.clone(),
            })
            .collect();
        let workspaces = self
            .workspaces
            .iter()
            .filter(|candidate| {
                candidate.id == workspace_id
                    || candidate
                        .tabs
                        .iter()
                        .any(|id| tabs.iter().any(|item| &item.tab.id == id))
            })
            .cloned()
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
            workspaces,
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
        self.closed_store.pending_structures()
    }

    pub fn finish_structure_close(&self, id: &str) -> Result<(), String> {
        self.closed_store.finish_structure_close(id)
    }

    pub fn discard_closed_structure(&self, id: &str) -> Result<(), String> {
        self.closed_store.discard_structure(id)
    }

    pub(crate) fn closed_structure_views(&self) -> Vec<ClosedStructureView> {
        match self.closed_store.structure_views() {
            Ok(views) => views,
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
                    && (existing.agent_type != tab.agent_type
                        || existing.agent_session_id != tab.agent_session_id
                        || existing.team_id != tab.team_id
                        || (existing.cwd.is_some() && existing.cwd != tab.cwd))
                {
                    return Err(format!("Tab identity {} is already in use", tab.id));
                }
            }
        }
        for workspace in &record.workspaces {
            if self
                .get_workspace(&workspace.id)
                .is_some_and(|existing| existing.cwd != workspace.cwd)
            {
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
        for saved in &record.workspaces {
            if staged.get_workspace(&saved.id).is_none() {
                let mut workspace = saved.clone();
                workspace.tabs.clear();
                workspace.active_tab_id = None;
                staged.workspaces.push(workspace);
            }
        }
        for team in &record.teams {
            staged.upsert_team(team.clone());
        }
        for tab in tabs {
            if let Some(existing) = staged.tabs.get_mut(&tab.id) {
                let same_restore = existing
                    .close_restore
                    .as_ref()
                    .and_then(|state| state.record_id.as_deref())
                    == Some(record.id.as_str());
                if !same_restore && existing.pty_id.is_none() {
                    *existing = tab;
                    continue;
                }
                if existing
                    .close_restore
                    .as_ref()
                    .and_then(|state| state.record_id.as_deref())
                    != Some(record.id.as_str())
                    && existing.task_binding.is_some()
                    && tab.task_binding.is_none()
                {
                    existing.role = tab.role;
                    existing.role_profile = tab.role_profile.clone();
                    existing.worker_id = None;
                    existing.task_binding = None;
                    existing.restorable_launch = tab.restorable_launch.clone();
                }
            } else {
                staged.tabs.insert(tab.id.clone(), tab);
            }
        }
        for saved in &record.workspaces {
            let workspace = staged
                .workspaces
                .iter_mut()
                .find(|workspace| workspace.id == saved.id)
                .ok_or("Restored workspace missing")?;
            let mut order: Vec<_> = saved
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
            workspace.active_tab_id = saved
                .active_tab_id
                .clone()
                .filter(|id| workspace.tabs.contains(id))
                .or_else(|| workspace.tabs.first().cloned());
        }
        let workspace = staged
            .get_workspace(&record.workspace.id)
            .ok_or("Restored workspace missing")?;
        let tab_id = workspace.active_tab_id.clone().unwrap_or_default();
        staged.active_workspace_id = Some(workspace.id.clone());
        staged.focused_tab_id = (!tab_id.is_empty()).then(|| tab_id.clone());
        transaction.commit(self, &[]);
        Ok((record.workspace.id.clone(), tab_id))
    }
}
