//! Normalized durable desk rows; runtime authority remains in the owning services.

mod mutations;
mod queries;
mod records;

pub(crate) use mutations::remove_team;

#[cfg(test)]
mod tests;

pub use records::{
    DeskAgentSessionIdentity, DeskLayoutNode, DeskLeafFields, DeskSnapshot, DeskState, DeskTab,
    DeskTeam, DeskTeamLifecycle, DeskWorkspace, MAX_DESK_CATALOGUE, MAX_DESK_LAYOUT_DEPTH,
    MAX_DESK_LAYOUT_NODES, MAX_DESK_LEAF_BYTES, MAX_DESK_TABS, MAX_DESK_TEAMS, MAX_DESK_WORKSPACES,
};

use crate::error::StorageError;
use crate::owner::Storage;
use crate::owner::WriteCx;
use crate::repositories::workspaces::WorkspaceRecord;
use crate::repositories::{Committed, Completion, Read, Revision};
use rusqlite::Connection;
use std::collections::HashSet;

fn retain_orphan_teams(snapshot: &mut DeskSnapshot) {
    let workspace_ids: HashSet<&str> = snapshot
        .workspaces
        .iter()
        .map(|workspace| workspace.record.id.as_str())
        .collect();
    snapshot
        .teams
        .retain(|team| workspace_ids.contains(team.workspace_id.as_str()));
}

pub(crate) fn write_import(
    cx: &mut WriteCx<'_>,
    snapshot: DeskSnapshot,
) -> Result<(), StorageError> {
    snapshot.validate_preserving_orphan_teams()?;
    mutations::import(cx, snapshot)
}

pub(crate) fn import_projection(conn: &Connection) -> Result<DeskSnapshot, StorageError> {
    queries::load(conn).map(|state| state.snapshot)
}

impl Storage {
    pub fn desk(&self) -> DeskRepository {
        DeskRepository {
            storage: self.clone(),
        }
    }
}

/// Typed desk commands are incremental. A snapshot is accepted only for first import or
/// additive restore; routine persistence uses entity-level operations and a flush barrier.
#[derive(Clone)]
pub struct DeskRepository {
    storage: Storage,
}

impl DeskRepository {
    pub fn snapshot(&self) -> Completion<Read<DeskState>> {
        self.storage.submit_read(|cx| queries::load(cx.conn))
    }

    /// Imports one original desk into an empty domain. It never merges into a live desk.
    pub fn import(&self, mut snapshot: DeskSnapshot) -> Completion<Committed<()>> {
        retain_orphan_teams(&mut snapshot);
        let cost = match snapshot.validate() {
            Ok(cost) => cost,
            Err(error) => return Completion::settled(Err(error)),
        };
        self.storage
            .submit_mutation(cost, move |cx| mutations::import(cx, snapshot))
    }

    /// Adds entities that are missing. Existing workspace, Team, tab and identity rows
    /// retain their current values and revisions.
    pub fn additive_restore(&self, mut snapshot: DeskSnapshot) -> Completion<Committed<()>> {
        retain_orphan_teams(&mut snapshot);
        let cost = match snapshot.validate() {
            Ok(cost) => cost,
            Err(error) => return Completion::settled(Err(error)),
        };
        self.storage
            .submit_mutation(cost, move |cx| mutations::additive_restore(cx, snapshot))
    }

    pub fn rename_workspace(
        &self,
        id: String,
        expect: Revision,
        name: String,
    ) -> Completion<Committed<()>> {
        let cost = id.len() + name.len();
        self.storage.submit_mutation(cost, move |cx| {
            mutations::rename_workspace(cx, &id, expect, name)
        })
    }

    pub fn update_workspace(
        &self,
        record: WorkspaceRecord,
        expect: Revision,
    ) -> Completion<Committed<()>> {
        let cost = record.cost();
        self.storage.submit_mutation(cost, move |cx| {
            mutations::update_workspace(cx, record, expect)
        })
    }

    pub fn rename_tab(
        &self,
        id: String,
        expect: Revision,
        title: String,
    ) -> Completion<Committed<()>> {
        let cost = id.len() + title.len();
        self.storage.submit_mutation(cost, move |cx| {
            mutations::rename_tab(cx, &id, expect, title)
        })
    }

    pub fn rename_team(
        &self,
        id: String,
        expect: Revision,
        name: String,
    ) -> Completion<Committed<()>> {
        let cost = id.len() + name.len();
        self.storage.submit_mutation(cost, move |cx| {
            mutations::rename_team(cx, &id, expect, name)
        })
    }

    pub fn set_team_lifecycle(
        &self,
        id: String,
        expect: Revision,
        lifecycle: DeskTeamLifecycle,
    ) -> Completion<Committed<()>> {
        let cost = id.len();
        self.storage.submit_mutation(cost, move |cx| {
            mutations::set_team_lifecycle(cx, &id, expect, lifecycle)
        })
    }

    pub fn select_workspace(
        &self,
        id: Option<String>,
        expect: Revision,
    ) -> Completion<Committed<()>> {
        self.storage
            .submit_mutation(id.as_ref().map_or(0, String::len), move |cx| {
                mutations::select_workspace(cx, id, expect)
            })
    }

    pub fn select_tab(
        &self,
        workspace_id: String,
        expect_workspace: Revision,
        tab_id: Option<String>,
    ) -> Completion<Committed<()>> {
        let cost = workspace_id.len() + tab_id.as_ref().map_or(0, String::len);
        self.storage.submit_mutation(cost, move |cx| {
            mutations::select_tab(cx, &workspace_id, expect_workspace, tab_id)
        })
    }

    pub fn remove_team(&self, id: String, expect: Revision) -> Completion<Committed<()>> {
        let cost = id.len();
        self.storage
            .submit_mutation(cost, move |cx| mutations::remove_team(cx, &id, expect))
    }

    pub fn reorder_tabs(
        &self,
        workspace_id: String,
        expect_order: Revision,
        order: Vec<String>,
    ) -> Completion<Committed<()>> {
        let cost = workspace_id.len() + order.iter().map(String::len).sum::<usize>();
        self.storage.submit_mutation(cost, move |cx| {
            mutations::reorder_tabs(cx, &workspace_id, expect_order, order)
        })
    }

    pub fn reorder_workspaces(
        &self,
        expect_order: Revision,
        order: Vec<String>,
    ) -> Completion<Committed<()>> {
        let cost = order.iter().map(String::len).sum::<usize>();
        self.storage.submit_mutation(cost, move |cx| {
            mutations::reorder_workspaces(cx, expect_order, order)
        })
    }

    pub fn close_tab(&self, id: String, expect: Revision) -> Completion<Committed<()>> {
        let cost = id.len();
        self.storage
            .submit_mutation(cost, move |cx| mutations::close_tab(cx, &id, expect))
    }

    pub fn close_workspace(&self, id: String, expect: Revision) -> Completion<Committed<()>> {
        let cost = id.len();
        self.storage
            .submit_mutation(cost, move |cx| mutations::close_workspace(cx, &id, expect))
    }

    /// FIFO writer barrier for save_session and shutdown callers.
    pub fn flush(&self) -> Completion<()> {
        self.storage.submit_exclusive(0, |_| Ok(()))
    }
}
