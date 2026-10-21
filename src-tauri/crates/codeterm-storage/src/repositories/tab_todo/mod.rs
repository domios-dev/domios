//! Incremental, generation-scoped checklist records in the shared Storage database.

mod lifecycle;
mod mutations;
mod own_write;
mod proposal;
mod queries;
mod records;

pub use records::{
    TabTodoAmendmentRecord, TabTodoChecklistRecord, TabTodoCountersRecord, TabTodoItemEventRecord,
    TabTodoItemRecord, TabTodoItemRefRecord, TabTodoNewItemRecord, TabTodoOwnWriteBase,
    TabTodoOwnWriteRecord, TabTodoStateChangeRecord, MAX_TAB_TODO_AUDIT_EVENTS, MAX_TAB_TODO_ITEMS,
    MAX_TAB_TODO_ITEM_EVENTS,
};

use crate::owner::Storage;
use crate::owner::WriteCx;
use crate::repositories::{Committed, Completion, Read};

impl Storage {
    pub fn tab_todos(&self) -> TabTodoRepository {
        TabTodoRepository {
            storage: self.clone(),
        }
    }
}

#[derive(Clone)]
pub struct TabTodoRepository {
    storage: Storage,
}

pub(crate) fn forget_team_in_transaction(
    cx: &mut WriteCx<'_>,
    team_id: &str,
) -> Result<(), crate::StorageError> {
    mutations::forget_team(cx, team_id.to_owned())
}

pub(crate) fn forget_workspace_in_transaction(
    cx: &mut WriteCx<'_>,
    workspace_id: &str,
) -> Result<(), crate::StorageError> {
    mutations::forget_workspace(cx, workspace_id.to_owned())
}

impl TabTodoRepository {
    pub fn get(&self, tab_id: String) -> Completion<Read<Option<TabTodoChecklistRecord>>> {
        self.storage
            .submit_read(move |cx| queries::load(cx.conn, &tab_id))
    }

    pub fn create(
        &self,
        tab_id: String,
        workspace_id: String,
        team_id: Option<String>,
        started_at: u64,
        items: Vec<TabTodoNewItemRecord>,
        truncated_item_ids: Vec<String>,
    ) -> Completion<Committed<TabTodoChecklistRecord>> {
        let cost = record_cost(&items) + tab_id.len() + 128;
        self.storage.submit_mutation(cost, move |cx| {
            mutations::create(
                cx,
                tab_id,
                workspace_id,
                team_id,
                started_at,
                items,
                truncated_item_ids,
            )
        })
    }

    pub fn append(
        &self,
        tab_id: String,
        generation: u64,
        actor: Option<String>,
        at: u64,
        items: Vec<TabTodoNewItemRecord>,
    ) -> Completion<Committed<TabTodoChecklistRecord>> {
        let cost = record_cost(&items) + tab_id.len() + 128;
        self.storage.submit_mutation(cost, move |cx| {
            mutations::append(cx, tab_id, generation, actor, at, items, true)
        })
    }

    /// The owning tab's declared checklist: creation, unlocked edits and state changes at once.
    pub fn write_own(
        &self,
        tab_id: String,
        workspace_id: String,
        team_id: Option<String>,
        at: u64,
        write: TabTodoOwnWriteRecord,
    ) -> Completion<Committed<TabTodoChecklistRecord>> {
        let cost = write.cost() + tab_id.len() + workspace_id.len() + 128;
        self.storage.submit_mutation(cost, move |cx| {
            own_write::write_own(cx, tab_id, workspace_id, team_id, at, write)
        })
    }

    pub fn tab_ids_with_items(&self) -> Completion<Read<Vec<String>>> {
        self.storage
            .submit_read(move |cx| own_write::tab_ids_with_items(cx.conn))
    }

    pub fn set_state(
        &self,
        tab_id: String,
        generation: u64,
        item_id: String,
        expected_revision: u64,
        state: String,
        reason: Option<String>,
        actor: Option<String>,
        at: u64,
    ) -> Completion<Committed<TabTodoChecklistRecord>> {
        let cost = tab_id.len()
            + item_id.len()
            + state.len()
            + reason.as_ref().map_or(0, String::len)
            + 128;
        self.storage.submit_mutation(cost, move |cx| {
            lifecycle::set_state(
                cx,
                tab_id,
                generation,
                item_id,
                expected_revision,
                state,
                reason,
                actor,
                at,
            )
        })
    }

    pub fn reopen(
        &self,
        tab_id: String,
        generation: u64,
        item_id: String,
        reason: String,
        actor: Option<String>,
        at: u64,
    ) -> Completion<Committed<TabTodoChecklistRecord>> {
        let cost = tab_id.len() + item_id.len() + reason.len() + 128;
        self.storage.submit_mutation(cost, move |cx| {
            lifecycle::reopen(cx, tab_id, generation, item_id, reason, actor, at)
        })
    }

    /// Moves a predecessor tab's checklist, attempts and history to its replacement.
    pub fn transfer(
        &self,
        source_tab_id: String,
        target_tab_id: String,
        workspace_id: String,
        team_id: Option<String>,
        actor: Option<String>,
        at: u64,
    ) -> Completion<Committed<Option<TabTodoChecklistRecord>>> {
        let cost = source_tab_id.len() + target_tab_id.len() + workspace_id.len() + 128;
        self.storage.submit_mutation(cost, move |cx| {
            lifecycle::transfer(
                cx,
                source_tab_id,
                target_tab_id,
                workspace_id,
                team_id,
                actor,
                at,
            )
        })
    }

    pub fn amend(
        &self,
        tab_id: String,
        generation: u64,
        item_id: String,
        expected_revision: u64,
        amendment: TabTodoAmendmentRecord,
        actor: Option<String>,
        at: u64,
    ) -> Completion<Committed<TabTodoChecklistRecord>> {
        let cost = tab_id.len() + item_id.len() + amendment.cost();
        self.storage.submit_mutation(cost, move |cx| {
            mutations::amend(
                cx,
                tab_id,
                generation,
                item_id,
                expected_revision,
                amendment,
                actor,
                at,
            )
        })
    }

    pub fn clear_one(
        &self,
        tab_id: String,
        generation: u64,
        item_id: String,
        expected_revision: u64,
        actor: Option<String>,
        at: u64,
    ) -> Completion<Committed<TabTodoChecklistRecord>> {
        let cost = tab_id.len() + item_id.len() + 128;
        self.storage.submit_mutation(cost, move |cx| {
            mutations::clear_one(
                cx,
                tab_id,
                generation,
                item_id,
                expected_revision,
                actor,
                at,
            )
        })
    }

    pub fn clear_all(
        &self,
        tab_id: String,
        generation: u64,
        actor: Option<String>,
        at: u64,
    ) -> Completion<Committed<TabTodoChecklistRecord>> {
        let cost = tab_id.len() + 128;
        self.storage.submit_mutation(cost, move |cx| {
            mutations::clear_all(cx, tab_id, generation, actor, at)
        })
    }

    pub fn reset(
        &self,
        tab_id: String,
        expected_generation: u64,
        workspace_id: String,
        team_id: Option<String>,
        started_at: u64,
        actor: Option<String>,
        items: Vec<TabTodoNewItemRecord>,
    ) -> Completion<Committed<TabTodoChecklistRecord>> {
        let cost = record_cost(&items) + tab_id.len() + 128;
        self.storage.submit_mutation(cost, move |cx| {
            mutations::reset(
                cx,
                tab_id,
                expected_generation,
                workspace_id,
                team_id,
                started_at,
                actor,
                items,
            )
        })
    }

    pub fn forget_tab(&self, tab_id: String) -> Completion<Committed<()>> {
        let cost = tab_id.len() + 64;
        self.storage
            .submit_mutation(cost, move |cx| mutations::forget_tab(cx, tab_id))
    }

    pub fn forget_team(&self, team_id: String) -> Completion<Committed<()>> {
        let cost = team_id.len() + 64;
        self.storage
            .submit_mutation(cost, move |cx| mutations::forget_team(cx, team_id))
    }

    pub fn forget_workspace(&self, workspace_id: String) -> Completion<Committed<()>> {
        let cost = workspace_id.len() + 64;
        self.storage.submit_mutation(cost, move |cx| {
            mutations::forget_workspace(cx, workspace_id)
        })
    }
}

fn record_cost(items: &[TabTodoNewItemRecord]) -> usize {
    items.iter().map(TabTodoNewItemRecord::cost).sum::<usize>() + 128
}

#[cfg(test)]
mod tests;
