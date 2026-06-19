//! Browser-style "recently closed" history + reopen, backed by sqlite.
//!
//! On every close the multiplexer records a [`ClosedGroup`] plus its
//! [`ClosedPane`] rows into [`ClosedPanesStore`]. A single tab/pane close is a
//! `Single` group; closing an orchestrator cascades its subtree into one
//! `Cascade` group (parent links preserved); closing a whole workspace records
//! a `Workspace` group with the tab list. Reopen re-materializes panes with the
//! SAME backing, marking each `restored_from_snapshot` so the existing restore
//! machinery takes over (PTY agent resume, chatBackend transcript reload).

mod store;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use super::MultiplexerState;
use crate::multiplexer::types::{ClosedPane, TitleLevel};

pub use store::{
    ClosedGroup, ClosedPanesStore, ClosedRow, GroupKind, CLOSED_GROUP_CAP, RECENTLY_CLOSED_CAP,
};

/// Bounded number of newest closed panes surfaced in the in-memory snapshot for
/// the sidebar. The durable history (sqlite) keeps far more; the hot snapshot
/// stays lean by carrying only this many.
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

/// A closed pane enriched for the sidebar: the per-workspace reopen `index`, an
/// `orphaned` flag (its workspace no longer exists), and the group it belongs to
/// (so the richer group UI can render it). Serialized into the snapshot.
#[derive(Debug, Clone, Serialize)]
pub struct ClosedPaneView {
    #[serde(flatten)]
    pub pane: ClosedPane,
    pub index: usize,
    pub orphaned: bool,
    pub group_id: String,
    pub group_kind: String,
}

impl MultiplexerState {
    /// Build a closed-pane record from a live pane, overlaying the tab/workspace
    /// context (the sidebar title/description and owning workspace). Returns
    /// `None` for unknown, hidden (ephemeral worker), or protected panes — those
    /// aren't user-closeable tabs and would only pollute the history.
    fn build_closed_pane(&self, pane_id: &str) -> Option<ClosedPane> {
        let pane = self.panes.get(pane_id)?;
        if pane.hidden || pane.protected {
            return None;
        }
        // The pane-derived fields come from one conversion so a new Pane field
        // can't silently desync the close path.
        let mut record = ClosedPane::from(pane);
        if let Some(ws) = self
            .find_workspace_for_pane(pane_id)
            .and_then(|id| self.workspaces.iter().find(|w| w.id == id))
        {
            record.workspace_id = ws.id.clone();
            record.workspace_name = ws.name.clone();
            // Prefer the tab's title/description — that's what the user sees in
            // the sidebar; pane.title can lag an OSC title update.
            if let Some(tab) = ws.tabs.iter().find(|t| t.pane_id == pane_id) {
                if !tab.title.is_empty() {
                    record.title = tab.title.clone();
                }
                if tab.description.is_some() {
                    record.description = tab.description.clone();
                }
            }
        }
        Some(record)
    }

    /// Record a single tab/pane close as a `Single` group.
    pub(super) fn record_single_close(&mut self, pane_id: &str) {
        let Some(record) = self.build_closed_pane(pane_id) else {
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

    /// Record an orchestrator + its spawned subtree as one group, preserving
    /// parent links. Falls back to a `Single` group when the pane has no
    /// children. Built BEFORE the panes are removed from the tree.
    pub(super) fn record_cascade_close(&mut self, root_pane_id: &str) {
        let subtree = self.collect_subtree(root_pane_id);
        let records: Vec<ClosedPane> = subtree
            .iter()
            .filter_map(|pid| self.build_closed_pane(pid))
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

    /// Record a whole workspace + its tab list as a `Workspace` group.
    pub(super) fn record_workspace_close(&mut self, workspace_id: &str) {
        let Some(ws) = self.workspaces.iter().find(|w| w.id == workspace_id) else {
            return;
        };
        let ws_name = ws.name.clone();
        let ws_cwd = ws.cwd.clone();
        let pane_ids: Vec<String> = ws.tabs.iter().map(|t| t.pane_id.clone()).collect();
        let records: Vec<ClosedPane> = pane_ids
            .iter()
            .filter_map(|pid| self.build_closed_pane(pid))
            .collect();
        if records.is_empty() {
            return;
        }
        let group = ClosedGroup {
            group_id: new_group_id(),
            kind: GroupKind::Workspace,
            label: ws_name.clone(),
            workspace_id: workspace_id.to_string(),
            workspace_name: ws_name,
            workspace_cwd: ws_cwd,
            closed_at: now_millis(),
        };
        let _ = self.closed_store.insert_group(&group, &records);
    }

    /// Pre-order list of a pane id followed by all of its descendants.
    fn collect_subtree(&self, root: &str) -> Vec<String> {
        let mut out = vec![root.to_string()];
        for child in self.get_children_ids(root) {
            out.extend(self.collect_subtree(&child));
        }
        out
    }

    /// A workspace's recently-closed panes (newest first). Backs the
    /// `list_recently_closed` command/REST/socket surface.
    pub fn list_recently_closed(&self, workspace_id: &str) -> Vec<ClosedPane> {
        self.closed_store.list_for_workspace(workspace_id)
    }

    /// Bounded newest-first view across all workspaces for the sidebar snapshot.
    pub(super) fn recent_closed_view(&self) -> Vec<ClosedPaneView> {
        let rows = self.closed_store.list_recent(RECENTLY_CLOSED_VIEW_LIMIT);
        let live: HashSet<&str> = self.workspaces.iter().map(|w| w.id.as_str()).collect();
        let mut per_ws: HashMap<String, usize> = HashMap::new();
        rows.into_iter()
            .map(
                |ClosedRow {
                     group_id,
                     group_kind,
                     pane,
                 }| {
                    let ws = pane.workspace_id.clone();
                    let index = {
                        let counter = per_ws.entry(ws.clone()).or_insert(0);
                        let i = *counter;
                        *counter += 1;
                        i
                    };
                    let orphaned = ws.is_empty() || !live.contains(ws.as_str());
                    ClosedPaneView {
                        index,
                        orphaned,
                        group_id,
                        group_kind: group_kind.as_str().to_string(),
                        pane,
                    }
                },
            )
            .collect()
    }

    /// Apply a closed record onto a freshly created tab's pane. Drives the
    /// existing restore path: PTY → resume-or-fresh, chatBackend → lazy reload.
    /// `chat_capable` carries the chat-UI-capable provider ids when the
    /// `default_new_agent_pane_chat` preference is on (empty otherwise) — a
    /// reopened agent pane re-applies that default, mirroring fresh spawns.
    fn materialize_closed_pane(
        &mut self,
        workspace_id: &str,
        record: &ClosedPane,
        chat_capable: &HashSet<String>,
    ) -> Result<(String, String), String> {
        let (tab_id, pane_id) = self.create_tab(workspace_id)?;
        if let Some(pane) = self.panes.get_mut(&pane_id) {
            pane.backing = record.backing.clone();
            pane.cwd = record.cwd.clone();
            pane.title = record.title.clone();
            pane.title_level = TitleLevel::User;
            pane.description = record.description.clone();
            pane.agent_type = record.agent_type.clone();
            // Re-lock identity so output-based re-detection can't flip the
            // provider before resume kicks in.
            pane.agent_type_locked = record.agent_type.is_some();
            pane.agent_session_id = record.agent_session_id.clone();
            pane.agent_session_model = record.model.clone();
            pane.restored_from_snapshot = true;
            // Reopen is a pane-creation path: a chat-capable agent pane opens in
            // chat when the preference is on. chatBackend panes carry no
            // `agent_type` (their chat UI is driven by `backing`) so they are
            // untouched here.
            pane.chat_view = record
                .agent_type
                .as_deref()
                .is_some_and(|at| chat_capable.contains(at));
        }
        let _ = self.rename_tab(workspace_id, &tab_id, &record.title);
        if let Some(ws) = self.workspaces.iter_mut().find(|w| w.id == workspace_id) {
            if let Some(tab) = ws.tabs.iter_mut().find(|t| t.id == tab_id) {
                tab.description = record.description.clone();
            }
        }
        Ok((tab_id, pane_id))
    }

    /// Reopen a recently-closed pane in `workspace_id`. `index` selects the
    /// entry (newest = 0, default). Returns `(tab_id, new_pane_id)`.
    pub fn reopen_closed_pane(
        &mut self,
        workspace_id: &str,
        index: Option<usize>,
        chat_capable: &HashSet<String>,
    ) -> Result<(String, String), String> {
        if !self.workspaces.iter().any(|w| w.id == workspace_id) {
            return Err(format!("Workspace not found: {}", workspace_id));
        }
        let count = self.closed_store.count_for_workspace(workspace_id);
        if count == 0 {
            return Err("No recently closed panes".to_string());
        }
        let idx = index.unwrap_or(0);
        if idx >= count {
            return Err(format!(
                "No recently closed pane at index {} (ring has {})",
                idx, count
            ));
        }
        let record = self
            .closed_store
            .take_in_workspace(workspace_id, idx)
            .ok_or("No recently closed pane")?;
        self.materialize_closed_pane(workspace_id, &record, chat_capable)
    }

    /// Resolve the workspace a restore should land in. If a LIVE workspace
    /// already points at the same folder (`cwd`), reopen into it rather than
    /// spawning a duplicate — a closed tab from `/foo` returns to the user's
    /// existing `/foo` workspace. Falls back to a fresh workspace from the
    /// stored name + cwd when nothing matches.
    fn restore_target_workspace(&mut self, name: String, cwd: Option<String>) -> String {
        if let Some(path) = cwd.as_deref() {
            if let Some(ws) = self.find_workspace_by_cwd(path) {
                return ws.id.clone();
            }
        }
        self.create_workspace(name, cwd).id
    }

    /// Reopen a pane whose original workspace was deleted: reuse a live
    /// workspace at the same path if one exists, else recreate from the stored
    /// name + cwd, then materialize the pane.
    /// Returns `(workspace_id, tab_id, pane_id)`.
    pub fn reopen_orphaned_pane(
        &mut self,
        pane_id: &str,
        chat_capable: &HashSet<String>,
    ) -> Result<(String, String, String), String> {
        let record = self
            .closed_store
            .take_by_pane_id(pane_id)
            .ok_or_else(|| format!("No orphaned entry for pane_id: {}", pane_id))?;
        let workspace_id =
            self.restore_target_workspace(record.workspace_name.clone(), record.cwd.clone());
        let (tab_id, new_pane_id) =
            self.materialize_closed_pane(&workspace_id, &record, chat_capable)?;
        Ok((workspace_id, tab_id, new_pane_id))
    }

    /// Reopen a whole closed group. A `Workspace` group recreates its workspace
    /// and every tab; a `Cascade`/`Single` group reopens into its origin
    /// workspace (recreated if gone). Hierarchy (`parent_pane_id`) is relinked
    /// across the freshly minted pane ids. Returns `(workspace_id, tab_id,
    /// first_pane_id)` for the activated tab.
    pub fn reopen_closed_group(
        &mut self,
        group_id: &str,
        chat_capable: &HashSet<String>,
    ) -> Result<(String, String, String), String> {
        let (group, panes) = self
            .closed_store
            .take_group(group_id)
            .ok_or_else(|| format!("No closed group: {}", group_id))?;
        if panes.is_empty() {
            return Err("Closed group has no panes".to_string());
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
                    // Origin workspace is gone: match a live one by path. Fall
                    // back to the lead pane's cwd when the group never recorded
                    // a workspace cwd (single/cascade closes).
                    let cwd = group
                        .workspace_cwd
                        .clone()
                        .or_else(|| panes.iter().find_map(|p| p.cwd.clone()));
                    self.restore_target_workspace(group.workspace_name.clone(), cwd)
                }
            }
        };

        let mut id_map: HashMap<String, String> = HashMap::new();
        let mut first: Option<(String, String)> = None;
        for record in &panes {
            let (tab_id, new_pane_id) =
                self.materialize_closed_pane(&workspace_id, record, chat_capable)?;
            id_map.insert(record.pane_id.clone(), new_pane_id.clone());
            if first.is_none() {
                first = Some((tab_id, new_pane_id));
            }
        }

        // Relink the subtree: old parent ids -> freshly minted pane ids.
        for record in &panes {
            let Some(old_parent) = &record.parent_pane_id else {
                continue;
            };
            if let (Some(new_self), Some(new_parent)) =
                (id_map.get(&record.pane_id), id_map.get(old_parent))
            {
                if let Some(pane) = self.panes.get_mut(new_self) {
                    pane.parent_pane_id = Some(new_parent.clone());
                }
            }
        }

        let (tab_id, pane_id) = first.ok_or("Closed group has no panes")?;
        Ok((workspace_id, tab_id, pane_id))
    }
}
