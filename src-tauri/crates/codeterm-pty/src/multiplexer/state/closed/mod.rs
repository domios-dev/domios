//! Browser-style "recently closed" history + reopen, backed by sqlite. Reopen
//! re-materializes tabs with the same backing, marking each
//! `restored_from_snapshot` so the existing restore machinery takes over.

mod store;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use super::MultiplexerState;
use crate::multiplexer::types::{ClosedTab, TabBacking, TabCloseReason, TitleLevel};

pub use store::{
    ClosedGroup, ClosedRow, ClosedTabsStore, GroupKind, CLOSED_GROUP_CAP, RECENTLY_CLOSED_CAP,
};

/// Cap on newest closed tabs carried in the hot snapshot; sqlite keeps far more.
pub const RECENTLY_CLOSED_VIEW_LIMIT: usize = 100;

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn new_group_id() -> String {
    ulid::Ulid::new().to_string()
}

/// A closed tab enriched for the sidebar: per-workspace reopen `index`, `orphaned` flag, and owning group.
#[derive(Debug, Clone, Serialize)]
pub struct ClosedTabView {
    #[serde(flatten)]
    pub tab: ClosedTab,
    pub index: usize,
    pub orphaned: bool,
    pub group_id: String,
    pub group_kind: String,
}

/// In-memory restore payload captured before lifecycle teardown mutates live tabs.
pub struct PreparedWorkspaceClose {
    workspace_id: String,
    reservation_id: String,
    tab_ids: Vec<String>,
    group: Option<ClosedGroup>,
    records: Vec<ClosedTab>,
}

/// Exclusive workspace-membership lease held across one tab's external teardown.
pub struct TabCloseReservation {
    pub(super) tab_id: String,
    workspace_id: String,
    reservation_id: String,
}

impl PreparedWorkspaceClose {
    /// Workspace whose exact tab set this payload covers.
    pub fn workspace_id(&self) -> &str {
        &self.workspace_id
    }

    /// Tab membership captured with the restore payload.
    pub fn tab_ids(&self) -> &[String] {
        &self.tab_ids
    }
}

impl MultiplexerState {
    /// Build a closed-tab record from a live tab, overlaying tab/workspace
    /// context. `None` for hidden/protected tabs — not user-closeable tabs —
    /// EXCEPT a policy-driven close of a task-owning worker (a hidden worker
    /// with a `worker_id`), which must stay restorable.
    fn build_closed_tab(&self, tab_id: &str, reason: TabCloseReason) -> Option<ClosedTab> {
        let tab = self.tabs.get(tab_id)?;
        let policy_worker = reason == TabCloseReason::PolicyWorker && tab.worker_id.is_some();
        if tab.protected {
            return None;
        }
        if tab.hidden && !policy_worker {
            return None;
        }
        // One conversion, so a new Tab field can't silently desync the close path.
        let mut record = ClosedTab::from(tab);
        if let Some(ws) = self
            .find_workspace_for_tab(tab_id)
            .and_then(|id| self.workspaces.iter().find(|w| w.id == id))
        {
            record.workspace_id = ws.id.clone();
            record.workspace_name = ws.name.clone();
        }
        Some(record)
    }

    /// Record a single tab/tab close as a `Single` group.
    pub(super) fn record_single_close(&mut self, tab_id: &str, reason: TabCloseReason) {
        let Some(record) = self.build_closed_tab(tab_id, reason) else {
            return;
        };
        let group = ClosedGroup {
            group_id: new_group_id(),
            kind: GroupKind::Single,
            label: record.title.clone(),
            workspace_id: record.workspace_id.clone(),
            workspace_name: record.workspace_name.clone(),
            workspace_cwd: None,
            closed_at: now_millis(),
        };
        let _ = self.closed_store.insert_group(&group, &[record]);
    }

    /// Record an orchestrator + its subtree as one group (parent links preserved).
    /// Falls back to `Single` with no children. Built before the tabs are removed.
    pub(super) fn record_cascade_close(&mut self, root_tab_id: &str) {
        let subtree = self.collect_subtree(root_tab_id);
        let records: Vec<ClosedTab> = subtree
            .iter()
            .filter_map(|pid| self.build_closed_tab(pid, TabCloseReason::User))
            .collect();
        if records.is_empty() {
            return;
        }
        let kind = if records.len() > 1 {
            GroupKind::Cascade
        } else {
            GroupKind::Single
        };
        let root = &records[0];
        let group = ClosedGroup {
            group_id: new_group_id(),
            kind,
            label: root.title.clone(),
            workspace_id: root.workspace_id.clone(),
            workspace_name: root.workspace_name.clone(),
            workspace_cwd: None,
            closed_at: now_millis(),
        };
        let _ = self.closed_store.insert_group(&group, &records);
    }

    /// Capture one restore group and reserve its membership until close or abort.
    pub fn reserve_workspace_close(
        &mut self,
        workspace_id: &str,
    ) -> Result<PreparedWorkspaceClose, String> {
        self.ensure_workspace_membership_mutable(workspace_id)?;
        let ws = self
            .workspaces
            .iter()
            .find(|w| w.id == workspace_id)
            .ok_or_else(|| format!("Workspace not found: {workspace_id}"))?;
        if ws.protected {
            return Err("Cannot close protected workspace".to_string());
        }
        let ws_name = ws.name.clone();
        let ws_cwd = ws.cwd.clone();
        let tab_ids = ws.tabs.clone();
        let records: Vec<ClosedTab> = tab_ids
            .iter()
            .filter_map(|pid| self.build_closed_tab(pid, TabCloseReason::User))
            .collect();
        let group = (!records.is_empty()).then(|| ClosedGroup {
            group_id: new_group_id(),
            kind: GroupKind::Workspace,
            label: ws_name.clone(),
            workspace_id: workspace_id.to_string(),
            workspace_name: ws_name,
            workspace_cwd: ws_cwd,
            closed_at: now_millis(),
        });
        let reservation_id = new_group_id();
        self.workspace_close_reservations
            .insert(workspace_id.to_string(), reservation_id.clone());
        Ok(PreparedWorkspaceClose {
            workspace_id: workspace_id.to_string(),
            reservation_id,
            tab_ids,
            group,
            records,
        })
    }

    /// Release a reservation when pre-close teardown cannot complete.
    pub fn abort_workspace_close(&mut self, prepared: &PreparedWorkspaceClose) {
        self.release_workspace_close_reservation(prepared);
    }

    /// Reserve the owning workspace before external tab teardown begins.
    pub fn reserve_tab_close(&mut self, tab_id: &str) -> Result<TabCloseReservation, String> {
        let workspace_id = self
            .find_workspace_for_tab(tab_id)
            .ok_or_else(|| format!("Tab not found in any workspace: {tab_id}"))?;
        self.ensure_workspace_membership_mutable(&workspace_id)?;
        let reservation_id = new_group_id();
        self.workspace_close_reservations
            .insert(workspace_id.clone(), reservation_id.clone());
        Ok(TabCloseReservation {
            tab_id: tab_id.to_string(),
            workspace_id,
            reservation_id,
        })
    }

    /// Release a tab-close reservation after pre-close failure.
    pub fn abort_tab_close(&mut self, reservation: &TabCloseReservation) {
        self.release_tab_close_reservation(reservation);
    }

    pub(super) fn ensure_workspace_membership_mutable(
        &self,
        workspace_id: &str,
    ) -> Result<(), String> {
        if self.workspace_close_reservations.contains_key(workspace_id) {
            return Err(format!("Workspace is closing: {workspace_id}"));
        }
        Ok(())
    }

    /// Refuse a tab mutation while its owning workspace is closing.
    pub fn ensure_tab_membership_mutable(&self, tab_id: &str) -> Result<(), String> {
        let workspace_id = self
            .find_workspace_for_tab(tab_id)
            .ok_or_else(|| format!("Tab not found in any workspace: {tab_id}"))?;
        self.ensure_workspace_membership_mutable(&workspace_id)
    }

    pub(super) fn workspace_close_is_reserved_by(&self, prepared: &PreparedWorkspaceClose) -> bool {
        self.workspace_close_reservations
            .get(&prepared.workspace_id)
            == Some(&prepared.reservation_id)
    }

    pub(super) fn release_workspace_close_reservation(
        &mut self,
        prepared: &PreparedWorkspaceClose,
    ) {
        if self.workspace_close_is_reserved_by(prepared) {
            self.workspace_close_reservations
                .remove(&prepared.workspace_id);
        }
    }

    pub(super) fn tab_close_is_reserved_by(&self, reservation: &TabCloseReservation) -> bool {
        self.workspace_close_reservations
            .get(&reservation.workspace_id)
            == Some(&reservation.reservation_id)
    }

    pub(super) fn release_tab_close_reservation(&mut self, reservation: &TabCloseReservation) {
        if self.tab_close_is_reserved_by(reservation) {
            self.workspace_close_reservations
                .remove(&reservation.workspace_id);
        }
    }

    pub(super) fn record_prepared_workspace_close(&mut self, prepared: &PreparedWorkspaceClose) {
        let Some(group) = prepared.group.as_ref() else {
            return;
        };
        let _ = self.closed_store.insert_group(group, &prepared.records);
    }

    /// Pre-order list of a tab id and its descendants; a `visited` set guards against a corrupted parent/child cycle.
    fn collect_subtree(&self, root: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut visited = HashSet::new();
        self.collect_subtree_into(root, &mut out, &mut visited);
        out
    }

    fn collect_subtree_into(
        &self,
        node: &str,
        out: &mut Vec<String>,
        visited: &mut HashSet<String>,
    ) {
        if !visited.insert(node.to_string()) {
            return;
        }
        out.push(node.to_string());
        for child in self.get_children_ids(node) {
            self.collect_subtree_into(&child, out, visited);
        }
    }

    /// A workspace's recently-closed tabs (newest first).
    pub fn list_recently_closed(&self, workspace_id: &str) -> Vec<ClosedTab> {
        self.closed_store.list_for_workspace(workspace_id)
    }

    /// Bounded newest-first view across all workspaces for the sidebar snapshot.
    pub(super) fn recent_closed_view(&self) -> Vec<ClosedTabView> {
        let rows = self.closed_store.list_recent(RECENTLY_CLOSED_VIEW_LIMIT);
        let live: HashSet<&str> = self.workspaces.iter().map(|w| w.id.as_str()).collect();
        let mut per_ws: HashMap<String, usize> = HashMap::new();
        rows.into_iter()
            .map(
                |ClosedRow {
                     group_id,
                     group_kind,
                     tab,
                 }| {
                    let ws = tab.workspace_id.clone();
                    let index = {
                        let counter = per_ws.entry(ws.clone()).or_insert(0);
                        let i = *counter;
                        *counter += 1;
                        i
                    };
                    let orphaned = ws.is_empty() || !live.contains(ws.as_str());
                    ClosedTabView {
                        index,
                        orphaned,
                        group_id,
                        group_kind: group_kind.as_str().to_string(),
                        tab,
                    }
                },
            )
            .collect()
    }

    /// Apply a closed record onto a fresh tab's tab, driving the restore path.
    fn materialize_closed_tab(
        &mut self,
        workspace_id: &str,
        record: &ClosedTab,
    ) -> Result<String, String> {
        let tab_id = self.create_tab(workspace_id)?;
        if let Some(tab) = self.tabs.get_mut(&tab_id) {
            tab.backing = record.backing.clone();
            tab.cwd = record.cwd.clone();
            tab.title = record.title.clone();
            tab.title_level = TitleLevel::User;
            tab.description = record.description.clone();
            tab.agent_type = record.agent_type.clone();
            // Re-lock identity so output-based re-detection can't flip the provider before resume kicks in.
            tab.agent_type_locked = record.agent_type.is_some();
            tab.agent_session_id = record.agent_session_id.clone();
            tab.role_profile = record.role_profile.clone();
            tab.worker_id = record.worker_id.clone();
            tab.agent_session_model = record.model.clone();
            tab.restored_from_snapshot = true;
            tab.chat_view =
                matches!(record.backing, TabBacking::ChatBackend { .. }) || record.chat_view;
        }
        Ok(tab_id)
    }

    /// Reopen a recently-closed tab in `workspace_id`. `index` selects the
    /// entry (newest = 0, default). Returns `(tab_id, new_tab_id)`.
    pub fn reopen_closed_tab(
        &mut self,
        workspace_id: &str,
        index: Option<usize>,
    ) -> Result<String, String> {
        if !self.workspaces.iter().any(|w| w.id == workspace_id) {
            return Err(format!("Workspace not found: {}", workspace_id));
        }
        let count = self.closed_store.count_for_workspace(workspace_id);
        if count == 0 {
            return Err("No recently closed Tabs".to_string());
        }
        let idx = index.unwrap_or(0);
        if idx >= count {
            return Err(format!(
                "No recently closed Tab at index {} (ring has {})",
                idx, count
            ));
        }
        // Peek → materialize → delete only on success, so a failing create can't drop the entry.
        let (row_id, record) = self
            .closed_store
            .peek_in_workspace(workspace_id, idx)
            .ok_or("No recently closed Tab")?;
        let res = self.materialize_closed_tab(workspace_id, &record)?;
        if let Err(e) = self.closed_store.delete_tab_row(row_id) {
            // History delete failed after materialize: a retry would duplicate. Surface it.
            log::warn!(
                "reopen_closed_tab: reopened Tab {} but failed to delete closed entry (row {row_id}): {e}",
                res
            );
        }
        Ok(res)
    }

    /// Resolve the workspace a restore lands in: reuse a live workspace at the same
    /// cwd rather than spawning a duplicate, else create one from the stored name + cwd.
    fn restore_target_workspace(&mut self, name: String, cwd: Option<String>) -> String {
        if let Some(path) = cwd.as_deref() {
            if let Some(ws) = self.find_workspace_by_cwd(path) {
                return ws.id.clone();
            }
        }
        self.create_workspace(name, cwd).id
    }

    /// Reopen a tab whose original workspace was deleted, into a live or recreated
    /// workspace at the same path.
    pub fn reopen_orphaned_tab(&mut self, tab_id: &str) -> Result<(String, String), String> {
        let (row_id, record) = self
            .closed_store
            .peek_by_tab_id(tab_id)
            .ok_or_else(|| format!("No orphaned entry for tab_id: {}", tab_id))?;
        let workspace_id =
            self.restore_target_workspace(record.workspace_name.clone(), record.cwd.clone());
        let new_tab_id = self.materialize_closed_tab(&workspace_id, &record)?;
        if let Err(e) = self.closed_store.delete_tab_row(row_id) {
            log::warn!(
                "reopen_orphaned_tab: reopened Tab {new_tab_id} but failed to delete closed entry (row {row_id}): {e}"
            );
        }
        Ok((workspace_id, new_tab_id))
    }

    /// Reopen a whole closed group, recreating its workspace if gone and relinking
    /// `parent_tab_id` across the new ids.
    pub fn reopen_closed_group(&mut self, group_id: &str) -> Result<(String, String), String> {
        // Materialize every tab first; delete the group only once they all succeed, so a mid-loop failure leaves it for retry.
        let (group, tabs) = self
            .closed_store
            .peek_group(group_id)
            .ok_or_else(|| format!("No closed group: {}", group_id))?;
        if tabs.is_empty() {
            return Err("Closed group has no Tabs".to_string());
        }

        let workspace_id = match group.kind {
            GroupKind::Workspace => self.restore_target_workspace(
                group.workspace_name.clone(),
                group.workspace_cwd.clone(),
            ),
            _ => {
                if self.workspaces.iter().any(|w| w.id == group.workspace_id) {
                    group.workspace_id.clone()
                } else {
                    // Origin workspace gone: match by path, falling back to the lead tab's cwd.
                    let cwd = group
                        .workspace_cwd
                        .clone()
                        .or_else(|| tabs.iter().find_map(|p| p.cwd.clone()));
                    self.restore_target_workspace(group.workspace_name.clone(), cwd)
                }
            }
        };

        let mut id_map: HashMap<String, String> = HashMap::new();
        let mut tabs_by_tab: HashMap<String, String> = HashMap::new();
        let mut first: Option<String> = None;
        for record in &tabs {
            let new_tab_id = self.materialize_closed_tab(&workspace_id, record)?;
            id_map.insert(record.tab_id.clone(), new_tab_id.clone());
            tabs_by_tab.insert(record.tab_id.clone(), new_tab_id.clone());
            if first.is_none() {
                first = Some(new_tab_id);
            }
        }

        // All tabs materialized: safe to drop the group from history.
        if let Err(e) = self.closed_store.delete_group(group_id) {
            // Delete failed: a retry would duplicate every tab. Surface it.
            log::warn!(
                "reopen_closed_group: reopened group {group_id} but failed to delete it from history: {e}"
            );
        }

        // Relink the subtree: old parent ids -> new tab ids.
        for record in &tabs {
            let Some(old_parent) = &record.parent_tab_id else {
                continue;
            };
            if let (Some(new_self), Some(new_parent)) =
                (id_map.get(&record.tab_id), id_map.get(old_parent))
            {
                if let Some(tab) = self.tabs.get_mut(new_self) {
                    tab.parent_tab_id = Some(new_parent.clone());
                }
            }
        }

        // Activate the cascade root (parent_tab_id == None), not whichever leaf the ordering surfaces first.
        let tab_id = tabs
            .iter()
            .find(|r| r.parent_tab_id.is_none())
            .and_then(|r| tabs_by_tab.get(&r.tab_id).cloned())
            .or(first)
            .ok_or("Closed group has no Tabs")?;
        Ok((workspace_id, tab_id))
    }
}
