use super::MultiplexerState;
use crate::multiplexer::types::AgentRole;

impl MultiplexerState {
    /// Set a pane's orchestration role with validation.
    pub fn set_pane_role(
        &mut self,
        pane_id: &str,
        role: AgentRole,
        parent_pane_id: Option<&str>,
    ) -> Result<(), String> {
        if !self.panes.contains_key(pane_id) {
            return Err(format!("Pane {} not found", pane_id));
        }

        match role {
            AgentRole::Orchestrator => {
                if parent_pane_id.is_some() {
                    return Err("Orchestrator cannot have a parent".to_string());
                }
                let existing = self
                    .panes
                    .get(pane_id)
                    .ok_or_else(|| format!("Pane {} not found", pane_id))?;
                if existing.parent_pane_id.is_some() {
                    return Err("Cannot become Orchestrator — already in a hierarchy".to_string());
                }
                let pane = self
                    .panes
                    .get_mut(pane_id)
                    .ok_or_else(|| format!("Pane {} not found", pane_id))?;
                pane.role = Some(AgentRole::Orchestrator);
                pane.parent_pane_id = None;
                pane.can_spawn = true;
                pane.depth = 1;
            }
            AgentRole::Worker => {
                let parent_id = parent_pane_id.ok_or("Worker requires a parent_pane_id")?;
                let parent = self
                    .panes
                    .get(parent_id)
                    .ok_or(format!("Parent pane {} not found", parent_id))?;
                if parent.role != Some(AgentRole::Orchestrator) {
                    return Err("Worker's parent must be an Orchestrator".to_string());
                }
                if !parent.can_spawn {
                    return Err("Parent does not allow spawning".to_string());
                }
                let parent_depth = parent.depth;
                let pane = self
                    .panes
                    .get_mut(pane_id)
                    .ok_or_else(|| format!("Pane {} not found", pane_id))?;
                pane.role = Some(AgentRole::Worker);
                pane.parent_pane_id = Some(parent_id.to_string());
                pane.can_spawn = false;
                pane.depth = parent_depth + 1;
            }
        }

        Ok(())
    }

    /// Clear a pane's orchestration role. Detaches a Worker from its
    /// Orchestrator (clears parent) or, for an Orchestrator, orphans its
    /// Workers. Works on any pane — `unlink` should be able to unlink the
    /// thing that's actually linked.
    pub fn clear_pane_role(&mut self, pane_id: &str) -> Result<(), String> {
        let pane = self
            .panes
            .get_mut(pane_id)
            .ok_or(format!("Pane {} not found", pane_id))?;
        pane.role = None;
        pane.parent_pane_id = None;
        pane.can_spawn = false;
        pane.depth = 0;
        self.orphan_children(pane_id);
        Ok(())
    }

    /// Set whether a pane can spawn child agents.
    pub fn set_can_spawn(&mut self, pane_id: &str, can_spawn: bool) -> Result<(), String> {
        let pane = self
            .panes
            .get_mut(pane_id)
            .ok_or(format!("Pane {} not found", pane_id))?;
        if pane.role.is_none() {
            return Err("Pane has no role".to_string());
        }
        match pane.role {
            Some(AgentRole::Orchestrator) => {
                pane.can_spawn = can_spawn;
                Ok(())
            }
            _ => Err("Only Orchestrator can spawn children".to_string()),
        }
    }

    /// Record the pane id of the caller that ran `agent spawn` for this pane.
    /// Independent of `parent_pane_id` — Orchestrators clear `parent_pane_id`
    /// but still need a route back to the agent that asked for them.
    pub fn set_spawned_by(&mut self, pane_id: &str, spawned_by: &str) -> Result<(), String> {
        let pane = self
            .panes
            .get_mut(pane_id)
            .ok_or(format!("Pane {} not found", pane_id))?;
        pane.spawned_by_pane_id = Some(spawned_by.to_string());
        Ok(())
    }

    /// Get IDs of direct children of a pane.
    pub fn get_children_ids(&self, pane_id: &str) -> Vec<String> {
        self.panes
            .values()
            .filter(|p| p.parent_pane_id.as_deref() == Some(pane_id))
            .map(|p| p.id.clone())
            .collect()
    }

    /// Remove role/parent from all children that point to the given pane.
    pub(super) fn orphan_children(&mut self, pane_id: &str) {
        let child_ids: Vec<String> = self.get_children_ids(pane_id);
        for cid in &child_ids {
            self.orphan_children(cid);
            if let Some(child) = self.panes.get_mut(cid) {
                child.role = None;
                child.parent_pane_id = None;
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
        let pane = mux.create_pane();
        (mux, pane)
    }

    #[test]
    fn orchestrator_defaults_can_spawn_true() {
        let (mut mux, _root) = MultiplexerState::new();
        let pane = mux.create_pane();
        mux.set_pane_role(&pane, AgentRole::Orchestrator, None)
            .expect("Orchestrator with no parent");
        let p = mux.get_pane(&pane).unwrap();
        assert_eq!(p.role, Some(AgentRole::Orchestrator));
        assert!(p.can_spawn, "Orchestrator must default to can_spawn=true");
        assert_eq!(p.depth, 1);
    }

    #[test]
    fn orchestrator_rejects_parent() {
        let (mut mux, _root) = MultiplexerState::new();
        let parent = mux.create_pane();
        let child = mux.create_pane();
        let err = mux
            .set_pane_role(&child, AgentRole::Orchestrator, Some(&parent))
            .unwrap_err();
        assert!(err.contains("Orchestrator"));
    }

    #[test]
    fn worker_defaults_can_spawn_false() {
        let (mut mux, root) = fresh();
        mux.set_pane_role(&root, AgentRole::Orchestrator, None)
            .unwrap();
        let worker = mux.create_pane();
        mux.set_pane_role(&worker, AgentRole::Worker, Some(&root))
            .expect("Worker under Orchestrator");
        let p = mux.get_pane(&worker).unwrap();
        assert!(!p.can_spawn, "Worker must default to can_spawn=false");
    }

    #[test]
    fn explicit_set_can_spawn_after_role_does_not_clobber_role() {
        // Pre-fix #42: callers chained set_pane_role + set_can_spawn(false) and accidentally reset Orchestrator's can_spawn to false on every spawn
        // Preserve old permissive role behavior: only override fields explicitly provided via --can-spawn.
        let (mut mux, _root) = MultiplexerState::new();
        let pane = mux.create_pane();
        mux.set_pane_role(&pane, AgentRole::Orchestrator, None)
            .unwrap();
        mux.set_can_spawn(&pane, false).unwrap();
        let p = mux.get_pane(&pane).unwrap();
        assert_eq!(p.role, Some(AgentRole::Orchestrator));
        assert!(!p.can_spawn);
    }

    #[test]
    fn worker_set_can_spawn_true_is_rejected() {
        let (mut mux, root) = fresh();
        mux.set_pane_role(&root, AgentRole::Orchestrator, None)
            .unwrap();
        let worker = mux.create_pane();
        mux.set_pane_role(&worker, AgentRole::Worker, Some(&root))
            .unwrap();
        let err = mux.set_can_spawn(&worker, true).unwrap_err();
        assert!(err.contains("Only Orchestrator"));
    }
}
