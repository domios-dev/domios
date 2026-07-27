use super::{MultiplexerState, TeamResolution};
use crate::multiplexer::types::AgentRole;
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

    /// Set a tab's orchestration role with validation.
    pub fn set_tab_role(
        &mut self,
        tab_id: &str,
        role: AgentRole,
        parent_tab_id: Option<&str>,
    ) -> Result<(), String> {
        if !self.tabs.contains_key(tab_id) {
            return Err(format!("Tab {} not found", tab_id));
        }

        match role {
            AgentRole::Orchestrator => {
                if parent_tab_id.is_some() {
                    return Err("Orchestrator cannot have a parent".to_string());
                }
                let existing = self
                    .tabs
                    .get(tab_id)
                    .ok_or_else(|| format!("Tab {} not found", tab_id))?;
                if existing.parent_tab_id.is_some() {
                    return Err("Cannot become Orchestrator — already in a hierarchy".to_string());
                }
                let tab = self
                    .tabs
                    .get_mut(tab_id)
                    .ok_or_else(|| format!("Tab {} not found", tab_id))?;
                tab.role = Some(AgentRole::Orchestrator);
                tab.role_profile = None;
                tab.parent_tab_id = None;
                tab.can_spawn = true;
                tab.depth = 1;
            }
            AgentRole::Manager => {
                let parent_id = parent_tab_id.ok_or("Manager requires a parent_tab_id")?;
                let parent = self
                    .tabs
                    .get(parent_id)
                    .ok_or(format!("Parent Tab {} not found", parent_id))?;
                if parent.role != Some(AgentRole::Orchestrator) {
                    return Err("Manager requires an Orchestrator parent".to_string());
                }
                if !parent.can_spawn {
                    return Err("Parent does not allow spawning".to_string());
                }
                let final_parent_depth = parent.depth;
                let team_id = self.resolve_or_mint_team_for_parent(parent_id)?.team_id;
                let tab = self
                    .tabs
                    .get_mut(tab_id)
                    .ok_or_else(|| format!("Tab {} not found", tab_id))?;
                tab.role = Some(AgentRole::Manager);
                tab.parent_tab_id = Some(parent_id.to_string());
                tab.can_spawn = false;
                tab.depth = final_parent_depth + 1;
                tab.team_id = Some(team_id);
            }
            AgentRole::Worker => {
                let parent_id = parent_tab_id.ok_or("Worker requires a parent_tab_id")?;
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
                let final_parent_depth = if parent_role == Some(AgentRole::Orchestrator) {
                    if !parent_can_spawn {
                        return Err("Parent does not allow spawning".to_string());
                    }
                    parent_depth
                } else {
                    if parent_has_parent {
                        return Err(
                            "Worker's parent is a leaf role; leaf roles cannot spawn workers"
                                .to_string(),
                        );
                    }
                    let parent = self
                        .tabs
                        .get_mut(parent_id)
                        .ok_or_else(|| format!("Parent Tab {} not found", parent_id))?;
                    parent.role = Some(AgentRole::Orchestrator);
                    parent.role_profile = None;
                    parent.parent_tab_id = None;
                    parent.can_spawn = true;
                    if parent.depth == 0 {
                        parent.depth = 1;
                    }
                    parent.depth
                };
                let team_id = self.resolve_or_mint_team_for_parent(parent_id)?.team_id;
                let tab = self
                    .tabs
                    .get_mut(tab_id)
                    .ok_or_else(|| format!("Tab {} not found", tab_id))?;
                tab.role = Some(AgentRole::Worker);
                tab.parent_tab_id = Some(parent_id.to_string());
                tab.can_spawn = false;
                tab.depth = final_parent_depth + 1;
                tab.team_id = Some(team_id);
            }
        }

        Ok(())
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
        match tab.role {
            Some(AgentRole::Orchestrator) => {
                tab.can_spawn = can_spawn;
                Ok(())
            }
            _ => Err("Only Orchestrator can spawn children".to_string()),
        }
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
mod tests {
    use super::*;

    fn fresh() -> (MultiplexerState, String) {
        let (mut mux, _root) = MultiplexerState::new();
        let tab = mux.create_tab(&mux.list_workspaces()[0].id.clone()).unwrap();
        (mux, tab)
    }

    #[test]
    fn orchestrator_defaults_can_spawn_true() {
        let (mut mux, _root) = MultiplexerState::new();
        let tab = mux.create_tab(&mux.list_workspaces()[0].id.clone()).unwrap();
        mux.set_tab_role(&tab, AgentRole::Orchestrator, None)
            .expect("Orchestrator with no parent");
        let p = mux.get_tab(&tab).unwrap();
        assert_eq!(p.role, Some(AgentRole::Orchestrator));
        assert!(p.can_spawn, "Orchestrator must default to can_spawn=true");
        assert_eq!(p.depth, 1);
    }

    #[test]
    fn orchestrator_rejects_parent() {
        let (mut mux, _root) = MultiplexerState::new();
        let parent = mux.create_tab(&mux.list_workspaces()[0].id.clone()).unwrap();
        let child = mux.create_tab(&mux.list_workspaces()[0].id.clone()).unwrap();
        let err = mux
            .set_tab_role(&child, AgentRole::Orchestrator, Some(&parent))
            .unwrap_err();
        assert!(err.contains("Orchestrator"));
    }

    #[test]
    fn worker_under_free_tab_auto_promotes_and_links() {
        let (mut mux, _root) = MultiplexerState::new();
        let parent = mux.create_tab(&mux.list_workspaces()[0].id.clone()).unwrap(); // free: role=None, can_spawn=false by default
        let worker = mux.create_tab(&mux.list_workspaces()[0].id.clone()).unwrap();
        mux.set_tab_role(&worker, AgentRole::Worker, Some(&parent))
            .expect("worker attaches to a free parent (auto-promote)");
        let p = mux.get_tab(&parent).unwrap();
        assert_eq!(
            p.role,
            Some(AgentRole::Orchestrator),
            "free parent auto-promoted to Orchestrator"
        );
        assert!(p.can_spawn);
        let w = mux.get_tab(&worker).unwrap();
        assert_eq!(
            w.parent_tab_id.as_deref(),
            Some(parent.as_str()),
            "parent link must persist"
        );
        assert_eq!(w.role, Some(AgentRole::Worker));
    }

    #[test]
    fn early_workers_under_uncommitted_orchestrator_all_link() {
        let (mut mux, _root) = MultiplexerState::new();
        let parent = mux.create_tab(&mux.list_workspaces()[0].id.clone()).unwrap();
        let w1 = mux.create_tab(&mux.list_workspaces()[0].id.clone()).unwrap();
        let w2 = mux.create_tab(&mux.list_workspaces()[0].id.clone()).unwrap();
        mux.set_tab_role(&w1, AgentRole::Worker, Some(&parent))
            .expect("first early child");
        mux.set_tab_role(&w2, AgentRole::Worker, Some(&parent))
            .expect("second early child");
        assert_eq!(
            mux.get_tab(&w1).unwrap().parent_tab_id.as_deref(),
            Some(parent.as_str())
        );
        assert_eq!(
            mux.get_tab(&w2).unwrap().parent_tab_id.as_deref(),
            Some(parent.as_str())
        );
        assert_eq!(
            mux.get_tab(&parent).unwrap().role,
            Some(AgentRole::Orchestrator)
        );
    }

    #[test]
    fn worker_defaults_can_spawn_false() {
        let (mut mux, root) = fresh();
        mux.set_tab_role(&root, AgentRole::Orchestrator, None)
            .unwrap();
        let worker = mux.create_tab(&mux.list_workspaces()[0].id.clone()).unwrap();
        mux.set_tab_role(&worker, AgentRole::Worker, Some(&root))
            .expect("Worker under Orchestrator");
        let p = mux.get_tab(&worker).unwrap();
        assert!(!p.can_spawn, "Worker must default to can_spawn=false");
    }

    #[test]
    fn manager_requires_parent() {
        let (mut mux, tab) = fresh();
        let err = mux
            .set_tab_role(&tab, AgentRole::Manager, None)
            .unwrap_err();
        assert!(err.contains("Manager requires a parent_tab_id"));
    }

    #[test]
    fn manager_requires_orchestrator_parent() {
        let (mut mux, root) = fresh();
        let worker = mux.create_tab(&mux.list_workspaces()[0].id.clone()).unwrap();
        mux.set_tab_role(&worker, AgentRole::Worker, Some(&root))
            .unwrap();
        let manager = mux.create_tab(&mux.list_workspaces()[0].id.clone()).unwrap();
        let err = mux
            .set_tab_role(&manager, AgentRole::Manager, Some(&worker))
            .unwrap_err();
        assert!(err.contains("Orchestrator parent"));
    }

    #[test]
    fn manager_under_orchestrator_is_leaf() {
        let (mut mux, root) = fresh();
        mux.set_tab_role(&root, AgentRole::Orchestrator, None)
            .unwrap();
        let manager = mux.create_tab(&mux.list_workspaces()[0].id.clone()).unwrap();
        mux.set_tab_role(&manager, AgentRole::Manager, Some(&root))
            .unwrap();
        let p = mux.get_tab(&manager).unwrap();
        assert_eq!(p.role, Some(AgentRole::Manager));
        assert_eq!(p.parent_tab_id.as_deref(), Some(root.as_str()));
        assert!(!p.can_spawn, "Manager must default to can_spawn=false");
    }

    #[test]
    fn manager_set_can_spawn_true_is_rejected() {
        let (mut mux, root) = fresh();
        mux.set_tab_role(&root, AgentRole::Orchestrator, None)
            .unwrap();
        let manager = mux.create_tab(&mux.list_workspaces()[0].id.clone()).unwrap();
        mux.set_tab_role(&manager, AgentRole::Manager, Some(&root))
            .unwrap();
        let err = mux.set_can_spawn(&manager, true).unwrap_err();
        assert!(err.contains("Only Orchestrator"));
    }

    #[test]
    fn manager_cannot_spawn_worker_child() {
        let (mut mux, root) = fresh();
        mux.set_tab_role(&root, AgentRole::Orchestrator, None)
            .unwrap();
        let manager = mux.create_tab(&mux.list_workspaces()[0].id.clone()).unwrap();
        mux.set_tab_role(&manager, AgentRole::Manager, Some(&root))
            .unwrap();
        let child = mux.create_tab(&mux.list_workspaces()[0].id.clone()).unwrap();
        let err = mux
            .set_tab_role(&child, AgentRole::Worker, Some(&manager))
            .unwrap_err();
        assert!(err.contains("leaf roles cannot spawn workers"));
    }

    #[test]
    fn explicit_set_can_spawn_after_role_does_not_clobber_role() {
        // Preserve old permissive role behavior: only override fields explicitly provided via --can-spawn.
        let (mut mux, _root) = MultiplexerState::new();
        let tab = mux.create_tab(&mux.list_workspaces()[0].id.clone()).unwrap();
        mux.set_tab_role(&tab, AgentRole::Orchestrator, None)
            .unwrap();
        mux.set_can_spawn(&tab, false).unwrap();
        let p = mux.get_tab(&tab).unwrap();
        assert_eq!(p.role, Some(AgentRole::Orchestrator));
        assert!(!p.can_spawn);
    }

    #[test]
    fn worker_set_can_spawn_true_is_rejected() {
        let (mut mux, root) = fresh();
        mux.set_tab_role(&root, AgentRole::Orchestrator, None)
            .unwrap();
        let worker = mux.create_tab(&mux.list_workspaces()[0].id.clone()).unwrap();
        mux.set_tab_role(&worker, AgentRole::Worker, Some(&root))
            .unwrap();
        let err = mux.set_can_spawn(&worker, true).unwrap_err();
        assert!(err.contains("Only Orchestrator"));
    }

    #[test]
    fn no_team_before_first_spawn() {
        let (mut mux, _root) = MultiplexerState::new();
        let tab = mux.create_tab(&mux.list_workspaces()[0].id.clone()).unwrap();
        assert_eq!(mux.team_count(), 0, "a roleless Tab mints no team");
        assert_eq!(
            mux.get_tab(&tab).unwrap().team_id,
            None,
            "roleless Tab carries no team membership"
        );
    }

    #[test]
    fn make_orchestrator_without_child_mints_nothing() {
        let (mut mux, _root) = MultiplexerState::new();
        let tab = mux.create_tab(&mux.list_workspaces()[0].id.clone()).unwrap();
        mux.set_tab_role(&tab, AgentRole::Orchestrator, None)
            .expect("explicit Orchestrator promotion");
        assert_eq!(
            mux.team_count(),
            0,
            "a promoted Orchestrator with zero children stays teamless"
        );
        assert_eq!(mux.get_tab(&tab).unwrap().team_id, None);
    }

    #[test]
    fn first_worker_spawn_mints_exactly_one_team() {
        let (mut mux, _root) = MultiplexerState::new();
        let parent = mux.create_tab(&mux.list_workspaces()[0].id.clone()).unwrap();
        let worker = mux.create_tab(&mux.list_workspaces()[0].id.clone()).unwrap();
        mux.set_tab_role(&worker, AgentRole::Worker, Some(&parent))
            .expect("worker under free parent auto-promotes and mints");
        assert_eq!(
            mux.team_count(),
            1,
            "first child link mints exactly one team"
        );
        let team_id = mux
            .get_tab(&parent)
            .unwrap()
            .team_id
            .clone()
            .expect("parent bound to the minted team");
        assert!(team_id.starts_with("team_"));
        assert_eq!(
            mux.get_tab(&worker).unwrap().team_id.as_deref(),
            Some(team_id.as_str()),
            "child shares the parent's team"
        );
        assert!(
            mux.get_team(&team_id).is_some(),
            "team registered in the store"
        );
    }

    #[test]
    fn explicit_orchestrator_then_first_spawn_mints_once() {
        let (mut mux, _root) = MultiplexerState::new();
        let parent = mux.create_tab(&mux.list_workspaces()[0].id.clone()).unwrap();
        mux.set_tab_role(&parent, AgentRole::Orchestrator, None)
            .expect("explicit Orchestrator");
        assert_eq!(mux.team_count(), 0, "promotion alone does not mint");
        let worker = mux.create_tab(&mux.list_workspaces()[0].id.clone()).unwrap();
        mux.set_tab_role(&worker, AgentRole::Worker, Some(&parent))
            .expect("first worker child link");
        assert_eq!(
            mux.team_count(),
            1,
            "team minted at the child link, not promotion"
        );
        let team_id = mux.get_tab(&parent).unwrap().team_id.clone().unwrap();
        assert_eq!(
            mux.get_tab(&worker).unwrap().team_id.as_deref(),
            Some(team_id.as_str())
        );
    }

    #[test]
    fn second_worker_inherits_single_team() {
        let (mut mux, _root) = MultiplexerState::new();
        let parent = mux.create_tab(&mux.list_workspaces()[0].id.clone()).unwrap();
        let w1 = mux.create_tab(&mux.list_workspaces()[0].id.clone()).unwrap();
        let w2 = mux.create_tab(&mux.list_workspaces()[0].id.clone()).unwrap();
        mux.set_tab_role(&w1, AgentRole::Worker, Some(&parent))
            .expect("first worker mints");
        mux.set_tab_role(&w2, AgentRole::Worker, Some(&parent))
            .expect("second worker inherits");
        assert_eq!(
            mux.team_count(),
            1,
            "parent team_id presence is the idempotency guard"
        );
        let team_id = mux.get_tab(&parent).unwrap().team_id.clone().unwrap();
        assert_eq!(
            mux.get_tab(&w1).unwrap().team_id.as_deref(),
            Some(team_id.as_str())
        );
        assert_eq!(
            mux.get_tab(&w2).unwrap().team_id.as_deref(),
            Some(team_id.as_str())
        );
    }

    #[test]
    fn manager_child_link_mints_team() {
        let (mut mux, _root) = MultiplexerState::new();
        let parent = mux.create_tab(&mux.list_workspaces()[0].id.clone()).unwrap();
        mux.set_tab_role(&parent, AgentRole::Orchestrator, None)
            .unwrap();
        let planner = mux.create_tab(&mux.list_workspaces()[0].id.clone()).unwrap();
        mux.set_tab_role(&planner, AgentRole::Manager, Some(&parent))
            .expect("planner (Manager) child link");
        assert_eq!(
            mux.team_count(),
            1,
            "Manager child link mints exactly one team"
        );
        let team_id = mux.get_tab(&parent).unwrap().team_id.clone().unwrap();
        assert_eq!(
            mux.get_tab(&planner).unwrap().team_id.as_deref(),
            Some(team_id.as_str()),
            "Manager child inherits/binds the parent's team"
        );
    }

    #[test]
    fn concurrent_first_spawns_single_team() {
        use parking_lot::Mutex;
        use std::sync::Arc;

        for _ in 0..100 {
            let (mut m, _root) = MultiplexerState::new();
            let parent = m.create_tab(&m.list_workspaces()[0].id.clone()).unwrap();
            let c1 = m.create_tab(&m.list_workspaces()[0].id.clone()).unwrap();
            let c2 = m.create_tab(&m.list_workspaces()[0].id.clone()).unwrap();
            let state = Arc::new(Mutex::new(m));

            let (s1, s2) = (Arc::clone(&state), Arc::clone(&state));
            let (p1, p2) = (parent.clone(), parent.clone());
            let (child1, child2) = (c1.clone(), c2.clone());
            let t1 = std::thread::spawn(move || {
                s1.lock()
                    .set_tab_role(&child1, AgentRole::Worker, Some(&p1))
                    .unwrap();
            });
            let t2 = std::thread::spawn(move || {
                s2.lock()
                    .set_tab_role(&child2, AgentRole::Worker, Some(&p2))
                    .unwrap();
            });
            t1.join().unwrap();
            t2.join().unwrap();

            let guard = state.lock();
            assert_eq!(
                guard.team_count(),
                1,
                "two concurrent first-spawns yield exactly one team"
            );
            let team_id = guard.get_tab(&parent).unwrap().team_id.clone().unwrap();
            assert_eq!(
                guard.get_tab(&c1).unwrap().team_id.as_deref(),
                Some(team_id.as_str())
            );
            assert_eq!(
                guard.get_tab(&c2).unwrap().team_id.as_deref(),
                Some(team_id.as_str())
            );
        }
    }
}
