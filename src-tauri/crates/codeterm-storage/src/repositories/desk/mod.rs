//! Normalized durable desk rows; runtime authority remains in the owning services.

mod mutations;
mod queries;
mod records;

#[cfg(test)]
mod tests;

pub use records::{
    DeskAgentSessionIdentity, DeskLayoutNode, DeskLeafFields, DeskSnapshot, DeskState, DeskTab,
    DeskTeam, DeskTeamLifecycle, DeskWorkspace, MAX_DESK_CATALOGUE, MAX_DESK_LAYOUT_DEPTH,
    MAX_DESK_LAYOUT_NODES, MAX_DESK_LEAF_BYTES, MAX_DESK_TABS, MAX_DESK_TEAMS, MAX_DESK_WORKSPACES,
};

use crate::owner::Storage;
use crate::repositories::{Committed, Completion, Read, Revision};

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
    pub fn import(&self, snapshot: DeskSnapshot) -> Completion<Committed<()>> {
        let cost = match snapshot.validate() {
            Ok(cost) => cost,
            Err(error) => return Completion::settled(Err(error)),
        };
        self.storage
            .submit_mutation(cost, move |cx| mutations::import(cx, snapshot))
    }

    /// Adds entities that are missing. Existing workspace, Team, tab and identity rows
    /// retain their current values and revisions.
    pub fn additive_restore(&self, snapshot: DeskSnapshot) -> Completion<Committed<()>> {
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
