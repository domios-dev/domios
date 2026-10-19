use super::records::{TabTodoChecklistRecord, TabTodoItemRecord};
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
        .prepare_cached("SELECT item_id, position, title, description, state, revision, link_json FROM tab_todo_items WHERE tab_id = ?1 AND generation = ?2 AND deleted = 0 ORDER BY position LIMIT 16")
        .map_err(|error| sqlite_error("prepare tab todo items", error))?;
    let rows = statement
        .query_map(params![tab_id, generation], decode_item)
        .map_err(|error| sqlite_error("read tab todo items", error))?;
    let mut items = Vec::new();
    for row in rows {
        items.push(row.map_err(|error| sqlite_error("decode tab todo item", error))??);
    }
    Ok(Some(TabTodoChecklistRecord {
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

fn decode_item(row: &Row<'_>) -> rusqlite::Result<Result<TabTodoItemRecord, StorageError>> {
    let position: i64 = row.get(1)?;
    let revision: i64 = row.get(5)?;
    Ok((|| {
        Ok(TabTodoItemRecord {
            id: row.get(0)?,
            position: u8::try_from(position).map_err(|_| StorageError::Corrupt {
                detail: "tab todo position is out of range".into(),
            })?,
            title: row.get(2)?,
            description: row.get(3)?,
            state: row.get(4)?,
            revision: from_sql_u64("todo item revision", revision)?,
            link_json: row.get(6)?,
        })
    })())
}
