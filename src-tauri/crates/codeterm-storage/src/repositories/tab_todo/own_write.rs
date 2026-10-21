//! An owning tab's declared checklist lands in one transaction or not at all.

use super::lifecycle;
use super::mutations;
use super::queries;
use super::records::{TabTodoChecklistRecord, TabTodoOwnWriteBase, TabTodoOwnWriteRecord};
use crate::error::StorageError;
use crate::owner::WriteCx;
use rusqlite::Connection;

pub(crate) fn write_own(
    cx: &mut WriteCx<'_>,
    tab_id: String,
    workspace_id: String,
    team_id: Option<String>,
    at: u64,
    write: TabTodoOwnWriteRecord,
) -> Result<TabTodoChecklistRecord, StorageError> {
    let actor = Some(tab_id.clone());
    let mut generation = write.generation;
    match write.base {
        TabTodoOwnWriteBase::Fresh => {
            generation = mutations::reset(
                cx,
                tab_id.clone(),
                write.generation,
                workspace_id,
                team_id,
                at,
                actor.clone(),
                write.appends,
            )?
            .generation;
        }
        TabTodoOwnWriteBase::Clear => {
            mutations::clear_all(cx, tab_id.clone(), generation, actor.clone(), at)?;
        }
        TabTodoOwnWriteBase::Edit => {
            for removal in write.removals {
                mutations::clear_one(
                    cx,
                    tab_id.clone(),
                    generation,
                    removal.item_id,
                    removal.expected_revision,
                    actor.clone(),
                    at,
                )?;
            }
            if !write.appends.is_empty() {
                mutations::append(
                    cx,
                    tab_id.clone(),
                    generation,
                    actor.clone(),
                    at,
                    write.appends,
                    false,
                )?;
            }
        }
    }
    for change in write.state_changes {
        lifecycle::set_state(
            cx,
            tab_id.clone(),
            generation,
            change.item_id,
            change.expected_revision,
            change.state,
            change.reason,
            actor.clone(),
            at,
        )?;
    }
    queries::load(cx.conn, &tab_id)?.ok_or_else(|| mutations::corrupt("written tab todo missing"))
}

/// Tabs whose current generation still holds live items.
pub(crate) fn tab_ids_with_items(conn: &Connection) -> Result<Vec<String>, StorageError> {
    let mut statement = conn
        .prepare_cached("SELECT DISTINCT h.tab_id FROM tab_todo_heads h JOIN tab_todo_items i ON i.tab_id = h.tab_id AND i.generation = h.generation AND i.deleted = 0 ORDER BY h.tab_id")
        .map_err(|error| crate::error::sqlite_error("prepare tab todo presence", error))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| crate::error::sqlite_error("read tab todo presence", error))?;
    rows.map(|row| {
        row.map_err(|error| crate::error::sqlite_error("decode tab todo presence", error))
    })
    .collect()
}
