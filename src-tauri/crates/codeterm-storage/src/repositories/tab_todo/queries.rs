use super::records::{
    TabTodoChecklistRecord, TabTodoCountersRecord, TabTodoItemEventRecord, TabTodoItemRecord,
    MAX_TAB_TODO_ITEMS, MAX_TAB_TODO_ITEM_EVENTS,
};
use crate::error::{sqlite_error, StorageError};
use crate::repositories::from_sql_u64;
use rusqlite::{params, Connection, OptionalExtension, Row};

pub(crate) fn load(
    conn: &Connection,
    tab_id: &str,
) -> Result<Option<TabTodoChecklistRecord>, StorageError> {
    let generation: Option<i64> = conn
        .prepare_cached("SELECT generation FROM tab_todo_heads WHERE tab_id = ?1")
        .and_then(|mut statement| statement.query_row([tab_id], |row| row.get(0)).optional())
        .map_err(|error| sqlite_error("read tab todo head", error))?;
    let Some(generation) = generation else {
        return Ok(None);
    };
    let header = conn
        .prepare_cached("SELECT workspace_id, team_id, started_at, revision, status FROM tab_todo_generations WHERE tab_id = ?1 AND generation = ?2")
        .and_then(|mut statement| statement.query_row(params![tab_id, generation], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?, row.get::<_, String>(4)?))
        }))
        .map_err(|error| sqlite_error("read tab todo generation", error))?;
    let mut statement = conn
        .prepare_cached(&format!("SELECT item_id, position, title, description, state, revision, link_json, locked, reason, attempt, started_count, completed_count, reopened_count, blocked_count, skipped_count, folded_events FROM tab_todo_items WHERE tab_id = ?1 AND generation = ?2 AND deleted = 0 ORDER BY position LIMIT {MAX_TAB_TODO_ITEMS}"))
        .map_err(|error| sqlite_error("prepare tab todo items", error))?;
    let rows = statement
        .query_map(params![tab_id, generation], decode_item)
        .map_err(|error| sqlite_error("read tab todo items", error))?;
    let mut items = Vec::new();
    for row in rows {
        items.push(row.map_err(|error| sqlite_error("decode tab todo item", error))??);
    }
    drop(statement);
    attach_events(conn, tab_id, generation, &mut items)?;
    Ok(Some(TabTodoChecklistRecord {
        proposal_json: conn.query_row("SELECT proposal_json FROM tab_todo_generations WHERE tab_id = ?1 AND generation = ?2", params![tab_id, generation], |row| row.get(0)).map_err(|error| sqlite_error("read checklist proposal", error))?,
        tab_id: tab_id.to_owned(),
        generation: from_sql_u64("todo generation", generation)?,
        workspace_id: header.0,
        team_id: header.1,
        started_at: from_sql_u64("todo start", header.2)?,
        revision: from_sql_u64("todo checklist revision", header.3)?,
        status: header.4,
        items,
    }))
}

/// Events of live items only; each item's log is already trimmed on write.
fn attach_events(
    conn: &Connection,
    tab_id: &str,
    generation: i64,
    items: &mut [TabTodoItemRecord],
) -> Result<(), StorageError> {
    let mut statement = conn
        .prepare_cached(&format!("SELECT e.item_id, e.kind, e.attempt, e.actor_tab_id, e.reason, e.happened_at FROM tab_todo_item_events e JOIN tab_todo_items i ON i.tab_id = e.tab_id AND i.generation = e.generation AND i.item_id = e.item_id AND i.deleted = 0 WHERE e.tab_id = ?1 AND e.generation = ?2 ORDER BY e.event_id LIMIT {}", MAX_TAB_TODO_ITEMS * MAX_TAB_TODO_ITEM_EVENTS))
        .map_err(|error| sqlite_error("prepare tab todo item events", error))?;
    let rows = statement
        .query_map(params![tab_id, generation], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })
        .map_err(|error| sqlite_error("read tab todo item events", error))?;
    for row in rows {
        let (item_id, kind, attempt, actor_tab_id, reason, happened_at) =
            row.map_err(|error| sqlite_error("decode tab todo item event", error))?;
        let Some(item) = items.iter_mut().find(|item| item.id == item_id) else {
            continue;
        };
        item.events.push(TabTodoItemEventRecord {
            kind,
            attempt: from_sql_u64("todo event attempt", attempt)?,
            actor_tab_id,
            reason,
            happened_at: from_sql_u64("todo event time", happened_at)?,
        });
    }
    Ok(())
}

fn decode_item(row: &Row<'_>) -> rusqlite::Result<Result<TabTodoItemRecord, StorageError>> {
    let id = row.get(0)?;
    let position: i64 = row.get(1)?;
    let title = row.get(2)?;
    let description = row.get(3)?;
    let state = row.get(4)?;
    let revision: i64 = row.get(5)?;
    let link_json = row.get(6)?;
    let locked = row.get(7)?;
    let reason = row.get(8)?;
    let attempt: i64 = row.get(9)?;
    let counts: [i64; 6] = [
        row.get(10)?,
        row.get(11)?,
        row.get(12)?,
        row.get(13)?,
        row.get(14)?,
        row.get(15)?,
    ];
    Ok((|| {
        let count = |index: usize| from_sql_u64("todo item counter", counts[index]);
        Ok(TabTodoItemRecord {
            id,
            position: u8::try_from(position).map_err(|_| StorageError::Corrupt {
                detail: "tab todo position is out of range".into(),
            })?,
            title,
            description,
            state,
            revision: from_sql_u64("todo item revision", revision)?,
            link_json,
            locked,
            reason,
            attempt: from_sql_u64("todo item attempt", attempt)?,
            counters: TabTodoCountersRecord {
                started: count(0)?,
                completed: count(1)?,
                reopened: count(2)?,
                blocked: count(3)?,
                skipped: count(4)?,
                folded_events: count(5)?,
            },
            events: Vec::new(),
        })
    })())
}
