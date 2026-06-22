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
            .map(|w| w.tabs.iter().map(|t| t.sort_order).max().unwrap_or(-1) + 1)
            .unwrap_or(0)
    }

    /// Reorder top-level tabs of a workspace. `ordered_tab_ids` is the full new
    /// order of that group; every id must be a top-level tab (its pane has no
    /// `parent_pane_id`) of this workspace. Assigns `sort_order = index`.
    pub fn reorder_tabs(
        &mut self,
        workspace_id: &str,
        ordered_tab_ids: &[String],
    ) -> Result<(), String> {
        let parent_of: std::collections::HashMap<String, Option<String>> = self
            .panes
            .iter()
            .map(|(id, p)| (id.clone(), p.parent_pane_id.clone()))
            .collect();
        let ws = self
            .workspaces
            .iter_mut()
            .find(|w| w.id == workspace_id)
            .ok_or_else(|| format!("Workspace not found: {}", workspace_id))?;
        for id in ordered_tab_ids {
            let tab = ws
                .tabs
                .iter()
                .find(|t| &t.id == id)
                .ok_or_else(|| format!("Tab not in workspace: {}", id))?;
            if parent_of
                .get(&tab.pane_id)
                .and_then(|p| p.clone())
                .is_some()
            {
                return Err(format!("Tab {} is a child, not a top-level tab", id));
            }
        }
        apply_order(ws, ordered_tab_ids);
        Ok(())
    }

    /// Reorder the children of a parent pane. `ordered_tab_ids` is the full new
    /// order of that sibling group; every id must be a tab whose pane's
    /// `parent_pane_id` equals `parent_pane_id`. Assigns `sort_order = index`.
    pub fn reorder_children(
        &mut self,
        parent_pane_id: &str,
        ordered_tab_ids: &[String],
    ) -> Result<(), String> {
        let parent_of: std::collections::HashMap<String, Option<String>> = self
            .panes
            .iter()
            .map(|(id, p)| (id.clone(), p.parent_pane_id.clone()))
            .collect();
        let ws = self
            .workspaces
            .iter_mut()
            .find(|w| w.tabs.iter().any(|t| ordered_tab_ids.contains(&t.id)))
            .ok_or_else(|| "No workspace owns the given tabs".to_string())?;
        for id in ordered_tab_ids {
            let tab = ws
                .tabs
                .iter()
                .find(|t| &t.id == id)
                .ok_or_else(|| format!("Tab not in workspace: {}", id))?;
            let is_child = parent_of
                .get(&tab.pane_id)
                .and_then(|p| p.as_deref())
                .map(|p| p == parent_pane_id)
                .unwrap_or(false);
            if !is_child {
                return Err(format!(
                    "Tab {} is not a child of pane {}",
                    id, parent_pane_id
                ));
            }
        }
        apply_order(ws, ordered_tab_ids);
        Ok(())
    }

    /// Reorder workspaces. `ordered_workspace_ids` is the full new order; every
    /// id must be a known workspace. Assigns `sort_order = index`.
    pub fn reorder_workspaces(&mut self, ordered_workspace_ids: &[String]) -> Result<(), String> {
        for (idx, id) in ordered_workspace_ids.iter().enumerate() {
            let ws = self
                .workspaces
                .iter_mut()
                .find(|w| &w.id == id)
                .ok_or_else(|| format!("Workspace not found: {}", id))?;
            ws.sort_order = idx as i32;
        }
        Ok(())
    }
}

fn apply_order(ws: &mut crate::multiplexer::types::Workspace, ordered_tab_ids: &[String]) {
    for (idx, id) in ordered_tab_ids.iter().enumerate() {
        if let Some(tab) = ws.tabs.iter_mut().find(|t| &t.id == id) {
            tab.sort_order = idx as i32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order_of(state: &MultiplexerState, ws_id: &str, tab_id: &str) -> i32 {
        state
            .get_workspace(ws_id)
            .unwrap()
            .tabs
            .iter()
            .find(|t| t.id == tab_id)
            .unwrap()
            .sort_order
    }

    #[test]
    fn new_tabs_get_increasing_ordinals() {
        let (mut state, _root) = MultiplexerState::new();
        let ws = state.list_workspaces()[0].id.clone();
        let (t1, _) = state.create_tab(&ws).unwrap();
        let (t2, _) = state.create_tab(&ws).unwrap();
        assert!(order_of(&state, &ws, &t1) < order_of(&state, &ws, &t2));
    }

    #[test]
    fn reorder_tabs_assigns_index_order() {
        let (mut state, _root) = MultiplexerState::new();
        let ws = state.list_workspaces()[0].id.clone();
        let (t1, _) = state.create_tab(&ws).unwrap();
        let (t2, _) = state.create_tab(&ws).unwrap();
        let (t3, _) = state.create_tab(&ws).unwrap();
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
        let (parent_tab, parent_pane) = state.create_tab(&ws).unwrap();
        let (child_tab, child_pane) = state.create_tab(&ws).unwrap();
        state.get_pane_mut(&child_pane).unwrap().parent_pane_id = Some(parent_pane.clone());
        // The child tab is not a top-level tab → rejected, no reorder applied.
        let err = state.reorder_tabs(&ws, &[child_tab.clone(), parent_tab.clone()]);
        assert!(err.is_err(), "a child tab must not pass the top-level op");
    }

    #[test]
    fn reorder_children_orders_only_same_parent_siblings() {
        let (mut state, _root) = MultiplexerState::new();
        let ws = state.list_workspaces()[0].id.clone();
        let (_pt, parent_pane) = state.create_tab(&ws).unwrap();
        let (c1, c1_pane) = state.create_tab(&ws).unwrap();
        let (c2, c2_pane) = state.create_tab(&ws).unwrap();
        state.get_pane_mut(&c1_pane).unwrap().parent_pane_id = Some(parent_pane.clone());
        state.get_pane_mut(&c2_pane).unwrap().parent_pane_id = Some(parent_pane.clone());
        state
            .reorder_children(&parent_pane, &[c2.clone(), c1.clone()])
            .unwrap();
        assert_eq!(order_of(&state, &ws, &c2), 0);
        assert_eq!(order_of(&state, &ws, &c1), 1);
    }

    #[test]
    fn reorder_children_rejects_foreign_parent() {
        let (mut state, _root) = MultiplexerState::new();
        let ws = state.list_workspaces()[0].id.clone();
        let (_pt, parent_pane) = state.create_tab(&ws).unwrap();
        let (top, _top_pane) = state.create_tab(&ws).unwrap();
        // `top` is top-level, not a child of parent_pane → must be rejected.
        assert!(state.reorder_children(&parent_pane, &[top]).is_err());
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

    #[test]
    fn sort_order_round_trips_through_session_snapshot() {
        let (mut state, _root) = MultiplexerState::new();
        let ws = state.list_workspaces()[0].id.clone();
        let (t1, _) = state.create_tab(&ws).unwrap();
        let (t2, _) = state.create_tab(&ws).unwrap();
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
