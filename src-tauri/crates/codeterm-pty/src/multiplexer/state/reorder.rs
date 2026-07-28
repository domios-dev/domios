//! Hierarchy-aware drag-to-reorder ops. Each writes a per-group `sort_order`
//! ordinal; the frontend sorts each sibling group by it. Ops are group-scoped
//! and validate membership so a reorder can never move an item across groups.

use super::MultiplexerState;

impl MultiplexerState {
    /// Next workspace ordinal (max + 1), so a freshly created workspace lands
    /// after its siblings.
    pub fn next_workspace_sort_order(&self) -> i32 {
        self.workspaces
            .iter()
            .map(|w| w.sort_order)
            .max()
            .unwrap_or(-1)
            + 1
    }

    /// Next tab ordinal within a workspace (max over all its tabs + 1). A new
    /// tab is parentless (top-level) at creation; a monotone per-workspace
    /// counter keeps insertion order stable within any sibling group it later
    /// joins.
    pub fn next_tab_sort_order(&self, workspace_id: &str) -> i32 {
        self.workspaces
            .iter()
            .find(|w| w.id == workspace_id)
            .map(|w| {
                w.tabs
                    .iter()
                    .filter_map(|tab_id| self.tabs.get(tab_id).map(|tab| tab.sort_order))
                    .max()
                    .unwrap_or(-1)
                    + 1
            })
            .unwrap_or(0)
    }

    /// Reorder top-level tabs of a workspace. `ordered_tab_ids` is the full new
    /// order of that group; every id must be a top-level tab (its tab has no
    /// `parent_tab_id`) of this workspace. Assigns `sort_order = index`.
    /// The General (protected) tab is clamped to index 0 — nothing reorders
    /// above it.
    pub fn reorder_tabs(
        &mut self,
        workspace_id: &str,
        ordered_tab_ids: &[String],
    ) -> Result<(), String> {
        let parent_of: std::collections::HashMap<String, Option<String>> = self
            .tabs
            .iter()
            .map(|(id, p)| (id.clone(), p.parent_tab_id.clone()))
            .collect();
        let protected_tabs: std::collections::HashSet<String> = self
            .tabs
            .iter()
            .filter(|(_, p)| p.protected)
            .map(|(id, _)| id.clone())
            .collect();
        let ws = self
            .workspaces
            .iter_mut()
            .find(|w| w.id == workspace_id)
            .ok_or_else(|| format!("Workspace not found: {}", workspace_id))?;
        for id in ordered_tab_ids {
            if !ws.tabs.contains(id) {
                return Err(format!("Tab not in workspace: {}", id));
            }
            if parent_of
                .get(id)
                .and_then(|parent| parent.clone())
                .is_some()
            {
                return Err(format!("Tab {} is a child, not a top-level tab", id));
            }
        }
        let final_order = pin_protected_tabs_first(ws, ordered_tab_ids, &protected_tabs);
        for (index, tab_id) in final_order.iter().enumerate() {
            if let Some(tab) = self.tabs.get_mut(tab_id) {
                tab.sort_order = index as i32;
            }
        }
        Ok(())
    }

    /// Reorder the children of a parent tab. `ordered_tab_ids` is the full new
    /// order of that sibling group; every id must be a tab whose tab's
    /// `parent_tab_id` equals `parent_tab_id`. Assigns `sort_order = index`.
    pub fn reorder_children(
        &mut self,
        parent_tab_id: &str,
        ordered_tab_ids: &[String],
    ) -> Result<(), String> {
        let parent_of: std::collections::HashMap<String, Option<String>> = self
            .tabs
            .iter()
            .map(|(id, p)| (id.clone(), p.parent_tab_id.clone()))
            .collect();
        let ws = self
            .workspaces
            .iter_mut()
            .find(|w| w.tabs.iter().any(|tab_id| ordered_tab_ids.contains(tab_id)))
            .ok_or_else(|| "No workspace owns the given tabs".to_string())?;
        for id in ordered_tab_ids {
            if !ws.tabs.contains(id) {
                return Err(format!("Tab not in workspace: {}", id));
            }
            let is_child = parent_of
                .get(id)
                .and_then(|p| p.as_deref())
                .map(|p| p == parent_tab_id)
                .unwrap_or(false);
            if !is_child {
                return Err(format!(
                    "Tab {} is not a child of Tab {}",
                    id, parent_tab_id
                ));
            }
        }
        for (index, tab_id) in ordered_tab_ids.iter().enumerate() {
            if let Some(tab) = self.tabs.get_mut(tab_id) {
                tab.sort_order = index as i32;
            }
        }
        Ok(())
    }

    /// Reorder workspaces. `ordered_workspace_ids` is the full new order; every
    /// id must be a known workspace. Assigns `sort_order = index`. The General
    /// (protected) workspace is clamped to index 0 — it never moves and nothing
    /// reorders above it.
    pub fn reorder_workspaces(&mut self, ordered_workspace_ids: &[String]) -> Result<(), String> {
        for id in ordered_workspace_ids {
            if !self.workspaces.iter().any(|w| &w.id == id) {
                return Err(format!("Workspace not found: {}", id));
            }
        }
        let pinned: Vec<String> = ordered_workspace_ids
            .iter()
            .filter(|id| self.is_protected_workspace(id))
            .cloned()
            .collect();
        let final_order: Vec<String> = pinned
            .iter()
            .cloned()
            .chain(
                ordered_workspace_ids
                    .iter()
                    .filter(|id| !pinned.contains(id))
                    .cloned(),
            )
            .collect();
        for (idx, id) in final_order.iter().enumerate() {
            if let Some(ws) = self.workspaces.iter_mut().find(|w| &w.id == id) {
                ws.sort_order = idx as i32;
            }
        }
        Ok(())
    }

    fn is_protected_workspace(&self, id: &str) -> bool {
        self.workspaces.iter().any(|w| w.id == id && w.protected)
    }
}

/// Reorder so any protected (General) tab leads the group, preserving the
/// requested relative order of the rest. Pin = locked at the front.
fn pin_protected_tabs_first(
    ws: &crate::multiplexer::types::Workspace,
    ordered_tab_ids: &[String],
    protected_tabs: &std::collections::HashSet<String>,
) -> Vec<String> {
    let is_pinned = |id: &String| ws.tabs.contains(id) && protected_tabs.contains(id);
    let pinned = ordered_tab_ids.iter().filter(|id| is_pinned(id)).cloned();
    let rest = ordered_tab_ids.iter().filter(|id| !is_pinned(id)).cloned();
    pinned.chain(rest).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order_of(state: &MultiplexerState, ws_id: &str, tab_id: &str) -> i32 {
        state.get_tab(tab_id).unwrap().sort_order
    }

    #[test]
    fn new_tabs_get_increasing_ordinals() {
        let (mut state, _root) = MultiplexerState::new();
        let ws = state.list_workspaces()[0].id.clone();
        let t1 = state.create_tab(&ws).unwrap();
        let t2 = state.create_tab(&ws).unwrap();
        assert!(order_of(&state, &ws, &t1) < order_of(&state, &ws, &t2));
    }

    #[test]
    fn reorder_tabs_assigns_index_order() {
        let (mut state, _root) = MultiplexerState::new();
        let ws = state.list_workspaces()[0].id.clone();
        let t1 = state.create_tab(&ws).unwrap();
        let t2 = state.create_tab(&ws).unwrap();
        let t3 = state.create_tab(&ws).unwrap();
        state
            .reorder_tabs(&ws, &[t3.clone(), t1.clone(), t2.clone()])
            .unwrap();
        assert_eq!(order_of(&state, &ws, &t3), 0);
        assert_eq!(order_of(&state, &ws, &t1), 1);
        assert_eq!(order_of(&state, &ws, &t2), 2);
    }

    #[test]
    fn reorder_tabs_rejects_a_child_tab() {
        let (mut state, _root) = MultiplexerState::new();
        let ws = state.list_workspaces()[0].id.clone();
        let parent_tab = state.create_tab(&ws).unwrap();
        let child_tab = state.create_tab(&ws).unwrap();
        state.get_tab_mut(&child_tab).unwrap().parent_tab_id = Some(parent_tab.clone());
        // The child tab is not a top-level tab → rejected, no reorder applied.
        let err = state.reorder_tabs(&ws, &[child_tab.clone(), parent_tab.clone()]);
        assert!(err.is_err(), "a child tab must not pass the top-level op");
    }

    #[test]
    fn reorder_children_orders_only_same_parent_siblings() {
        let (mut state, _root) = MultiplexerState::new();
        let ws = state.list_workspaces()[0].id.clone();
        let parent_tab = state.create_tab(&ws).unwrap();
        let c1 = state.create_tab(&ws).unwrap();
        let c2 = state.create_tab(&ws).unwrap();
        state.get_tab_mut(&c1).unwrap().parent_tab_id = Some(parent_tab.clone());
        state.get_tab_mut(&c2).unwrap().parent_tab_id = Some(parent_tab.clone());
        state
            .reorder_children(&parent_tab, &[c2.clone(), c1.clone()])
            .unwrap();
        assert_eq!(order_of(&state, &ws, &c2), 0);
        assert_eq!(order_of(&state, &ws, &c1), 1);
    }

    #[test]
    fn reorder_children_rejects_foreign_parent() {
        let (mut state, _root) = MultiplexerState::new();
        let ws = state.list_workspaces()[0].id.clone();
        let parent_tab = state.create_tab(&ws).unwrap();
        let top = state.create_tab(&ws).unwrap();
        // `top` is top-level, not a child of parent_tab → must be rejected.
        assert!(state.reorder_children(&parent_tab, &[top]).is_err());
    }

    #[test]
    fn reorder_workspaces_assigns_index_order() {
        let (mut state, _root) = MultiplexerState::new();
        let a = state.list_workspaces()[0].id.clone();
        let b = state.create_workspace("B".into(), None).id;
        let c = state.create_workspace("C".into(), None).id;
        state
            .reorder_workspaces(&[c.clone(), a.clone(), b.clone()])
            .unwrap();
        assert_eq!(state.get_workspace(&c).unwrap().sort_order, 0);
        assert_eq!(state.get_workspace(&a).unwrap().sort_order, 1);
        assert_eq!(state.get_workspace(&b).unwrap().sort_order, 2);
    }

    fn protect_workspace(state: &mut MultiplexerState, ws_id: &str) {
        state.set_workspace_protected(ws_id, true);
    }

    #[test]
    fn reorder_cannot_place_a_workspace_above_general() {
        let (mut state, _root) = MultiplexerState::new();
        let general = state.list_workspaces()[0].id.clone();
        protect_workspace(&mut state, &general);
        let b = state.create_workspace("B".into(), None).id;
        let c = state.create_workspace("C".into(), None).id;
        // Try to drag B and C above General.
        state
            .reorder_workspaces(&[b.clone(), c.clone(), general.clone()])
            .unwrap();
        assert_eq!(
            state.get_workspace(&general).unwrap().sort_order,
            0,
            "General is clamped to index 0"
        );
        // The rest keep their requested relative order, shifted below General.
        assert_eq!(state.get_workspace(&b).unwrap().sort_order, 1);
        assert_eq!(state.get_workspace(&c).unwrap().sort_order, 2);
    }

    #[test]
    fn general_workspace_stays_at_index_zero_when_dragged_down() {
        let (mut state, _root) = MultiplexerState::new();
        let general = state.list_workspaces()[0].id.clone();
        protect_workspace(&mut state, &general);
        let b = state.create_workspace("B".into(), None).id;
        // Try to move General itself to the bottom.
        state
            .reorder_workspaces(&[b.clone(), general.clone()])
            .unwrap();
        assert_eq!(state.get_workspace(&general).unwrap().sort_order, 0);
        assert_eq!(state.get_workspace(&b).unwrap().sort_order, 1);
    }

    #[test]
    fn reorder_cannot_place_a_tab_above_the_general_tab() {
        let (mut state, _root) = MultiplexerState::new();
        let ws = state.list_workspaces()[0].id.clone();
        // The first tab's tab is the General (protected) tab.
        let general_tab = state.get_workspace(&ws).unwrap().tabs[0].clone();
        let general_tab = state.get_workspace(&ws).unwrap().tabs[0].clone();
        state.get_tab_mut(&general_tab).unwrap().protected = true;
        let t1 = state.create_tab(&ws).unwrap();
        let t2 = state.create_tab(&ws).unwrap();
        // Try to drag t1 and t2 above the General tab.
        state
            .reorder_tabs(&ws, &[t1.clone(), t2.clone(), general_tab.clone()])
            .unwrap();
        assert_eq!(
            order_of(&state, &ws, &general_tab),
            0,
            "General tab is clamped to index 0"
        );
        assert_eq!(order_of(&state, &ws, &t1), 1);
        assert_eq!(order_of(&state, &ws, &t2), 2);
    }

    #[test]
    fn move_to_front_pins_general_sort_order_to_zero() {
        let (mut state, _root) = MultiplexerState::new();
        let a = state.list_workspaces()[0].id.clone();
        let general = state.create_workspace("General".into(), None).id;
        let c = state.create_workspace("C".into(), None).id;
        protect_workspace(&mut state, &general);
        state.move_workspace_to_front(&general);
        assert_eq!(state.get_workspace(&general).unwrap().sort_order, 0);
        assert_eq!(state.get_workspace(&a).unwrap().sort_order, 1);
        assert_eq!(state.get_workspace(&c).unwrap().sort_order, 2);
    }

    #[test]
    fn pinned_general_round_trips_through_session_snapshot() {
        let (mut state, _root) = MultiplexerState::new();
        let general = state.list_workspaces()[0].id.clone();
        protect_workspace(&mut state, &general);
        let b = state.create_workspace("B".into(), None).id;
        state
            .reorder_workspaces(&[b.clone(), general.clone()])
            .unwrap();
        let restored = MultiplexerState::from_session_snapshot(&state.to_session_snapshot());
        assert_eq!(restored.get_workspace(&general).unwrap().sort_order, 0);
        assert_eq!(restored.get_workspace(&b).unwrap().sort_order, 1);
    }

    #[test]
    fn sort_order_round_trips_through_session_snapshot() {
        let (mut state, _root) = MultiplexerState::new();
        let ws = state.list_workspaces()[0].id.clone();
        let t1 = state.create_tab(&ws).unwrap();
        let t2 = state.create_tab(&ws).unwrap();
        let b = state.create_workspace("B".into(), None).id;
        state.reorder_tabs(&ws, &[t2.clone(), t1.clone()]).unwrap();
        state.reorder_workspaces(&[b.clone(), ws.clone()]).unwrap();

        let restored = MultiplexerState::from_session_snapshot(&state.to_session_snapshot());
        assert_eq!(order_of(&restored, &ws, &t2), 0);
        assert_eq!(order_of(&restored, &ws, &t1), 1);
        assert_eq!(restored.get_workspace(&b).unwrap().sort_order, 0);
        assert_eq!(restored.get_workspace(&ws).unwrap().sort_order, 1);
    }
}
