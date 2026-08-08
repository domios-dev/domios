use super::{MultiplexerState, TeamResolution};
use crate::multiplexer::types::AgentRole;
use codeterm_types::agent::role_kind;
use codeterm_types::team::Team;

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

impl MultiplexerState {
    /// Resolve or mint a parent's Team while holding the mux write lock.
    pub fn resolve_or_mint_team_for_parent(
        &mut self,
        parent_id: &str,
    ) -> Result<TeamResolution, String> {
        let existing = self
            .tabs
            .get(parent_id)
            .ok_or_else(|| format!("Tab {parent_id} not found"))?
            .team_id
            .clone();
        if let Some(id) = existing {
            return Ok(TeamResolution {
                team_id: id,
                minted: false,
            });
        }
        let workspace_id = self.find_workspace_for_tab(parent_id).unwrap_or_default();
        let team = Team::new(workspace_id, now_millis());
        let team_id = team.id.clone();
        self.upsert_team(team);
        self.tabs
            .get_mut(parent_id)
            .expect("Tab checked before team mint")
            .team_id = Some(team_id.clone());
        Ok(TeamResolution {
            team_id,
            minted: true,
        })
    }

    pub fn rollback_minted_team_for_parent(&mut self, parent_id: &str, team_id: &str) {
        let is_current = self
            .tabs
            .get(parent_id)
            .and_then(|tab| tab.team_id.as_deref())
            == Some(team_id);
        if is_current {
            self.tabs
                .get_mut(parent_id)
                .expect("Tab checked before team rollback")
                .team_id = None;
            self.remove_team(team_id);
        }
    }

    /// Placement follows the kind's capability descriptor, never an enum arm:
    /// an anchoring kind takes no parent, every other kind needs a spawner.
    pub fn set_tab_role(
        &mut self,
        tab_id: &str,
        role: AgentRole,
        parent_tab_id: Option<&str>,
    ) -> Result<(), String> {
        let role_profile = self
            .tabs
            .get(tab_id)
            .ok_or_else(|| format!("Tab {} not found", tab_id))?
            .role_profile
            .clone();
        self.set_tab_role_kind(tab_id, role, role_profile.as_deref(), parent_tab_id)
    }

    /// Set a tab's role and role kind together. The kind is what carries
    /// authority, so it must be applied in the same transition as the role.
    pub fn set_tab_role_kind(
        &mut self,
        tab_id: &str,
        role: AgentRole,
        role_profile: Option<&str>,
        parent_tab_id: Option<&str>,
    ) -> Result<(), String> {
        if !self.tabs.contains_key(tab_id) {
            return Err(format!("Tab {} not found", tab_id));
        }
        let kind_id = role_kind::kind_id(Some(role), role_profile)
            .ok_or("Manager requires a named role_profile")?
            .to_string();
        let capability = role_kind::capability_for_kind(&kind_id);
        let label = role_kind::kind_label(Some(role), role_profile).unwrap_or(role.label());

        if capability.anchors_team() {
            if parent_tab_id.is_some() {
                return Err(format!("{label} cannot have a parent"));
            }
            let existing = self
                .tabs
                .get(tab_id)
                .ok_or_else(|| format!("Tab {} not found", tab_id))?;
            if existing.parent_tab_id.is_some() {
                return Err(format!("Cannot become {label} — already in a hierarchy"));
            }
            let tab = self
                .tabs
                .get_mut(tab_id)
                .ok_or_else(|| format!("Tab {} not found", tab_id))?;
            tab.role = Some(role);
            tab.role_profile = Some(kind_id);
            tab.parent_tab_id = None;
            tab.can_spawn = true;
            tab.depth = 1;
            return Ok(());
        }

        let parent_id = parent_tab_id.ok_or_else(|| format!("{label} requires a parent_tab_id"))?;
        let parent_depth = self.place_under_spawning_parent(parent_id, role)?;
        let team_id = self.resolve_or_mint_team_for_parent(parent_id)?.team_id;
        let tab = self
            .tabs
            .get_mut(tab_id)
            .ok_or_else(|| format!("Tab {} not found", tab_id))?;
        tab.role = Some(role);
        tab.role_profile = match role {
            AgentRole::Manager => Some(kind_id),
            AgentRole::Worker => role_profile.map(str::to_string),
        };
        tab.parent_tab_id = Some(parent_id.to_string());
        tab.can_spawn = false;
        tab.depth = parent_depth + 1;
        tab.team_id = Some(team_id);
        Ok(())
    }

    /// Place a tab as the default Team anchor (the `orchestrator` kind).
    pub fn set_orchestrator_role(&mut self, tab_id: &str) -> Result<(), String> {
        self.set_tab_role_kind(
            tab_id,
            AgentRole::Manager,
            Some(role_kind::builtins::ORCHESTRATOR),
            None,
        )
    }

    /// Resolve the parent a leaf role attaches to, auto-promoting a free tab to
    /// the default anchor. Returns the parent's final depth.
    fn place_under_spawning_parent(
        &mut self,
        parent_id: &str,
        role: AgentRole,
    ) -> Result<u8, String> {
        let (parent_role, parent_has_parent, parent_can_spawn, parent_depth) = {
            let parent = self
                .tabs
                .get(parent_id)
                .ok_or(format!("Parent Tab {} not found", parent_id))?;
            (
                parent.role,
                parent.parent_tab_id.is_some(),
                parent.can_spawn,
                parent.depth,
            )
        };
        if parent_can_spawn {
            return Ok(parent_depth);
        }
        // Only a Worker link auto-promotes a free parent; a Manager placement
        // must name an already-spawning anchor.
        if role != AgentRole::Worker {
            return Err(format!(
                "{} requires a spawning parent",
                role_kind::kind_label(Some(role), None).unwrap_or(role.label())
            ));
        }
        if parent_role.is_some() || parent_has_parent {
            return Err(
                "Worker's parent is a leaf role; leaf roles cannot spawn workers".to_string(),
            );
        }
        let parent = self
            .tabs
            .get_mut(parent_id)
            .ok_or_else(|| format!("Parent Tab {} not found", parent_id))?;
        parent.role = Some(AgentRole::Manager);
        parent.role_profile = Some(role_kind::builtins::ORCHESTRATOR.to_string());
        parent.parent_tab_id = None;
        parent.can_spawn = true;
        if parent.depth == 0 {
            parent.depth = 1;
        }
        Ok(parent.depth)
    }

    /// Clear a tab's orchestration role. Detaches a Worker from its
    /// Orchestrator (clears parent) or, for an Orchestrator, orphans its
    /// Workers. Works on any tab — `unlink` should be able to unlink the
    /// thing that's actually linked.
    pub fn clear_tab_role(&mut self, tab_id: &str) -> Result<(), String> {
        let tab = self
            .tabs
            .get_mut(tab_id)
            .ok_or(format!("Tab {} not found", tab_id))?;
        tab.role = None;
        tab.role_profile = None;
        tab.parent_tab_id = None;
        tab.can_spawn = false;
        tab.depth = 0;
        self.orphan_children(tab_id);
        Ok(())
    }

    /// Repoint a tab's parent edge. No-op returning false if the tab is absent.
    /// Overwrites any stale parent, keeping depth = parent.depth + 1 when known.
    pub fn repoint_parent(&mut self, tab_id: &str, new_parent: &str) -> bool {
        let parent_depth = self.tabs.get(new_parent).map(|p| p.depth);
        match self.tabs.get_mut(tab_id) {
            Some(tab) => {
                tab.parent_tab_id = Some(new_parent.to_string());
                if let Some(d) = parent_depth {
                    tab.depth = d + 1;
                }
                true
            }
            None => false,
        }
    }

    /// Set whether a tab can spawn child agents.
    pub fn set_can_spawn(&mut self, tab_id: &str, can_spawn: bool) -> Result<(), String> {
        let tab = self
            .tabs
            .get_mut(tab_id)
            .ok_or(format!("Tab {} not found", tab_id))?;
        if tab.role.is_none() {
            return Err("Tab has no role".to_string());
        }
        if !role_kind::has_team_authority(tab.role, tab.role_profile.as_deref()) {
            return Err("Only managing roles can spawn children".to_string());
        }
        tab.can_spawn = can_spawn;
        Ok(())
    }

    /// Semantic Worker identity for a cross-mesh tab whose hierarchy lives on
    /// the origin: Worker role and leaf capability, no target-local parent.
    pub fn set_detached_worker_role(
        &mut self,
        tab_id: &str,
        role_profile: Option<&str>,
    ) -> Result<(), String> {
        let tab = self
            .tabs
            .get_mut(tab_id)
            .ok_or(format!("Tab {} not found", tab_id))?;
        tab.role = Some(AgentRole::Worker);
        tab.role_profile = role_profile.map(str::to_string);
        tab.parent_tab_id = None;
        tab.can_spawn = false;
        tab.depth = 2;
        Ok(())
    }

    /// Record the tab id of the caller that ran `agent spawn` for this tab.
    /// Independent of `parent_tab_id` — Orchestrators clear `parent_tab_id`
    /// but still need a route back to the agent that asked for them.
    pub fn set_spawned_by(&mut self, tab_id: &str, spawned_by: &str) -> Result<(), String> {
        let tab = self
            .tabs
            .get_mut(tab_id)
            .ok_or(format!("Tab {} not found", tab_id))?;
        tab.spawned_by_tab_id = Some(spawned_by.to_string());
        Ok(())
    }

    /// Get IDs of direct children of a tab.
    pub fn get_children_ids(&self, tab_id: &str) -> Vec<String> {
        self.tabs
            .values()
            .filter(|p| p.parent_tab_id.as_deref() == Some(tab_id))
            .map(|p| p.id.clone())
            .collect()
    }

    /// Remove role/parent from all children that point to the given tab.
    pub(super) fn orphan_children(&mut self, tab_id: &str) {
        let child_ids: Vec<String> = self.get_children_ids(tab_id);
        for cid in &child_ids {
            self.orphan_children(cid);
            if let Some(child) = self.tabs.get_mut(cid) {
                child.role = None;
                child.role_profile = None;
                child.parent_tab_id = None;
                child.can_spawn = false;
                child.depth = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests;
