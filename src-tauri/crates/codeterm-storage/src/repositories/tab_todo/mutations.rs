use super::queries;
use super::records::{
    validate_item_id, validate_items, validate_tab_id, validate_team_id, validate_workspace_id,
    TabTodoAmendmentRecord, TabTodoChecklistRecord, TabTodoNewItemRecord,
};
use crate::error::{sqlite_error, StorageError};
use crate::owner::WriteCx;
use crate::repositories::{to_sql_u64, Revision};
use rusqlite::{params, Connection, OptionalExtension};

pub(crate) fn create(
    cx: &mut WriteCx<'_>,
    tab_id: String,
    workspace_id: String,
    team_id: Option<String>,
    started_at: u64,
    items: Vec<TabTodoNewItemRecord>,
    truncated_item_ids: Vec<String>,
) -> Result<TabTodoChecklistRecord, StorageError> {
    validate_tab_id(&tab_id)?;
    validate_workspace_id(&workspace_id)?;
    validate_team_id(team_id.as_deref())?;
    validate_items(&items)?;
    let item_ids: std::collections::HashSet<&str> =
        items.iter().map(|item| item.id.as_str()).collect();
    if truncated_item_ids
        .iter()
        .any(|id| !item_ids.contains(id.as_str()))
    {
        return Err(StorageError::invalid(
            "truncated todo audit",
            "references an item outside the checklist",
        ));
    }
    let started_at = to_sql_u64("todo start", started_at)?;
    let existing: Option<i64> = cx
        .conn
        .prepare_cached("SELECT generation FROM tab_todo_heads WHERE tab_id = ?1")
        .and_then(|mut statement| statement.query_row([&tab_id], |row| row.get(0)).optional())
        .map_err(|error| sqlite_error("check tab todo existence", error))?;
    if existing.is_some() {
        return Err(StorageError::invalid(
            "todo checklist",
            "already exists; reset it",
        ));
    }
    insert_generation(
        cx,
        &tab_id,
        1,
        &workspace_id,
        team_id.as_deref(),
        started_at,
    )?;
    cx.write(
        "INSERT INTO tab_todo_heads (tab_id, generation) VALUES (?1, 1)",
        [&tab_id],
    )?;
    for (position, item) in items.iter().enumerate() {
        insert_item(cx, &tab_id, 1, position, item)?;
        audit(
            cx,
            &tab_id,
            1,
            Some(&item.id),
            None,
            "create",
            Some(1),
            started_at,
        )?;
    }
    for item_id in truncated_item_ids {
        audit(
            cx,
            &tab_id,
            1,
            Some(&item_id),
            None,
            "title_truncated",
            Some(1),
            started_at,
        )?;
    }
    queries::load(cx.conn, &tab_id)?.ok_or_else(|| corrupt("created tab todo missing"))
}

pub(crate) fn append(
    cx: &mut WriteCx<'_>,
    tab_id: String,
    generation: u64,
    actor_tab_id: Option<String>,
    at: u64,
    items: Vec<TabTodoNewItemRecord>,
) -> Result<TabTodoChecklistRecord, StorageError> {
    validate_tab_id(&tab_id)?;
    validate_actor(actor_tab_id.as_deref())?;
    validate_items(&items)?;
    let generation = to_sql_u64("todo generation", generation)?;
    let at = to_sql_u64("todo audit time", at)?;
    require_mutable_generation(cx.conn, &tab_id, generation)?;
    let (count, next) = live_item_count(cx.conn, &tab_id, generation)?;
    if count + items.len() > 16 || next + items.len() > 16 {
        return Err(StorageError::invalid(
            "todo items",
            "would exceed the 16-item ordered list bound",
        ));
    }
    for (offset, item) in items.iter().enumerate() {
        insert_item(cx, &tab_id, generation as u64, next + offset, item)?;
        audit(
            cx,
            &tab_id,
            generation,
            Some(&item.id),
            actor_tab_id.as_deref(),
            "append",
            Some(1),
            at,
        )?;
    }
    update_lifecycle(cx, &tab_id, generation)?;
    touch_generation(cx, &tab_id, generation)?;
    queries::load(cx.conn, &tab_id)?.ok_or_else(|| corrupt("appended tab todo missing"))
}

pub(crate) fn set_state(
    cx: &mut WriteCx<'_>,
    tab_id: String,
    generation: u64,
    item_id: String,
    expected_revision: u64,
    state: String,
    actor_tab_id: Option<String>,
    at: u64,
) -> Result<TabTodoChecklistRecord, StorageError> {
    validate_tab_id(&tab_id)?;
    validate_item_id(&item_id)?;
    validate_actor(actor_tab_id.as_deref())?;
    validate_state(&state)?;
    let generation = to_sql_u64("todo generation", generation)?;
    let expected_revision = to_sql_u64("todo item revision", expected_revision)?;
    let at = to_sql_u64("todo audit time", at)?;
    require_mutable_generation(cx.conn, &tab_id, generation)?;
    let changed = cx.write("UPDATE tab_todo_items SET state = ?5, revision = revision + 1 WHERE tab_id = ?1 AND generation = ?2 AND item_id = ?3 AND revision = ?4 AND deleted = 0", params![tab_id, generation, item_id, expected_revision, state])?;
    check_item_write(
        cx.conn,
        &tab_id,
        generation,
        &item_id,
        expected_revision,
        changed,
    )?;
    update_lifecycle(cx, &tab_id, generation)?;
    touch_generation(cx, &tab_id, generation)?;
    audit(
        cx,
        &tab_id,
        generation,
        Some(&item_id),
        actor_tab_id.as_deref(),
        "state_change",
        Some(expected_revision as u64 + 1),
        at,
    )?;
    queries::load(cx.conn, &tab_id)?.ok_or_else(|| corrupt("updated tab todo missing"))
}

pub(crate) fn amend(
    cx: &mut WriteCx<'_>,
    tab_id: String,
    generation: u64,
    item_id: String,
    expected_revision: u64,
    amendment: TabTodoAmendmentRecord,
    actor_tab_id: Option<String>,
    at: u64,
) -> Result<TabTodoChecklistRecord, StorageError> {
    validate_tab_id(&tab_id)?;
    validate_item_id(&item_id)?;
    validate_actor(actor_tab_id.as_deref())?;
    amendment.validate()?;
    let generation = to_sql_u64("todo generation", generation)?;
    let expected_revision = to_sql_u64("todo item revision", expected_revision)?;
    let at = to_sql_u64("todo audit time", at)?;
    require_mutable_generation(cx.conn, &tab_id, generation)?;
    let changed = cx.write("UPDATE tab_todo_items SET title = ?5, description = ?6, link_json = ?7, revision = revision + 1 WHERE tab_id = ?1 AND generation = ?2 AND item_id = ?3 AND revision = ?4 AND deleted = 0", params![tab_id, generation, item_id, expected_revision, amendment.title, amendment.description, amendment.link_json])?;
    check_item_write(
        cx.conn,
        &tab_id,
        generation,
        &item_id,
        expected_revision,
        changed,
    )?;
    touch_generation(cx, &tab_id, generation)?;
    audit(
        cx,
        &tab_id,
        generation,
        Some(&item_id),
        actor_tab_id.as_deref(),
        "amend",
        Some(expected_revision as u64 + 1),
        at,
    )?;
    queries::load(cx.conn, &tab_id)?.ok_or_else(|| corrupt("amended tab todo missing"))
}

pub(crate) fn clear_one(
    cx: &mut WriteCx<'_>,
    tab_id: String,
    generation: u64,
    item_id: String,
    expected_revision: u64,
    actor_tab_id: Option<String>,
    at: u64,
) -> Result<TabTodoChecklistRecord, StorageError> {
    validate_tab_id(&tab_id)?;
    validate_item_id(&item_id)?;
    validate_actor(actor_tab_id.as_deref())?;
    let generation = to_sql_u64("todo generation", generation)?;
    let expected_revision = to_sql_u64("todo item revision", expected_revision)?;
    let at = to_sql_u64("todo audit time", at)?;
    require_mutable_generation(cx.conn, &tab_id, generation)?;
    let changed = cx.write("UPDATE tab_todo_items SET deleted = 1, revision = revision + 1 WHERE tab_id = ?1 AND generation = ?2 AND item_id = ?3 AND revision = ?4 AND deleted = 0", params![tab_id, generation, item_id, expected_revision])?;
    check_item_write(
        cx.conn,
        &tab_id,
        generation,
        &item_id,
        expected_revision,
        changed,
    )?;
    compact_live_positions(cx, &tab_id, generation)?;
    update_lifecycle(cx, &tab_id, generation)?;
    touch_generation(cx, &tab_id, generation)?;
    audit(
        cx,
        &tab_id,
        generation,
        Some(&item_id),
        actor_tab_id.as_deref(),
        "clear_one",
        Some(expected_revision as u64 + 1),
        at,
    )?;
    queries::load(cx.conn, &tab_id)?.ok_or_else(|| corrupt("cleared tab todo missing"))
}

pub(crate) fn clear_all(
    cx: &mut WriteCx<'_>,
    tab_id: String,
    generation: u64,
    actor_tab_id: Option<String>,
    at: u64,
) -> Result<TabTodoChecklistRecord, StorageError> {
    validate_tab_id(&tab_id)?;
    validate_actor(actor_tab_id.as_deref())?;
    let generation = to_sql_u64("todo generation", generation)?;
    let at = to_sql_u64("todo audit time", at)?;
    require_mutable_generation(cx.conn, &tab_id, generation)?;
    cx.write("UPDATE tab_todo_items SET deleted = 1, revision = revision + 1 WHERE tab_id = ?1 AND generation = ?2 AND deleted = 0", params![tab_id, generation])?;
    set_status(cx, &tab_id, generation, "cleared")?;
    audit(
        cx,
        &tab_id,
        generation,
        None,
        actor_tab_id.as_deref(),
        "clear_all",
        None,
        at,
    )?;
    queries::load(cx.conn, &tab_id)?.ok_or_else(|| corrupt("cleared tab todo missing"))
}

pub(crate) fn reset(
    cx: &mut WriteCx<'_>,
    tab_id: String,
    expected_generation: u64,
    workspace_id: String,
    team_id: Option<String>,
    started_at: u64,
    actor_tab_id: Option<String>,
    items: Vec<TabTodoNewItemRecord>,
) -> Result<TabTodoChecklistRecord, StorageError> {
    validate_tab_id(&tab_id)?;
    validate_workspace_id(&workspace_id)?;
    validate_team_id(team_id.as_deref())?;
    validate_actor(actor_tab_id.as_deref())?;
    validate_items(&items)?;
    let expected_generation = to_sql_u64("todo generation", expected_generation)?;
    let started_at = to_sql_u64("todo start", started_at)?;
    require_current_generation(cx.conn, &tab_id, expected_generation)?;
    let next = expected_generation
        .checked_add(1)
        .ok_or_else(|| StorageError::invalid("todo generation", "overflows"))?;
    cx.write("UPDATE tab_todo_generations SET status = 'reset', revision = revision + 1 WHERE tab_id = ?1 AND generation = ?2", params![tab_id, expected_generation])?;
    audit(
        cx,
        &tab_id,
        expected_generation,
        None,
        actor_tab_id.as_deref(),
        "reset",
        None,
        started_at,
    )?;
    cx.write(
        "DELETE FROM tab_todo_items WHERE tab_id = ?1 AND generation = ?2",
        params![tab_id, expected_generation],
    )?;
    insert_generation(
        cx,
        &tab_id,
        next as u64,
        &workspace_id,
        team_id.as_deref(),
        started_at,
    )?;
    let changed = cx.write(
        "UPDATE tab_todo_heads SET generation = ?2 WHERE tab_id = ?1 AND generation = ?3",
        params![tab_id, next, expected_generation],
    )?;
    if changed != 1 {
        return Err(stale_generation(
            &tab_id,
            Some(expected_generation),
            current_generation(cx.conn, &tab_id)?,
        ));
    }
    for (position, item) in items.iter().enumerate() {
        insert_item(cx, &tab_id, next as u64, position, item)?;
    }
    audit(
        cx,
        &tab_id,
        next,
        None,
        actor_tab_id.as_deref(),
        "reset",
        None,
        started_at,
    )?;
    queries::load(cx.conn, &tab_id)?.ok_or_else(|| corrupt("reset tab todo missing"))
}

pub(crate) fn forget_tab(cx: &mut WriteCx<'_>, tab_id: String) -> Result<(), StorageError> {
    validate_tab_id(&tab_id)?;
    cx.write("DELETE FROM tab_todo_items WHERE tab_id = ?1", [&tab_id])?;
    cx.write(
        "DELETE FROM tab_todo_generations WHERE tab_id = ?1",
        [&tab_id],
    )?;
    cx.write("DELETE FROM tab_todo_audit WHERE tab_id = ?1", [&tab_id])?;
    cx.write("DELETE FROM tab_todo_heads WHERE tab_id = ?1", [&tab_id])?;
    Ok(())
}

pub(crate) fn forget_team(cx: &mut WriteCx<'_>, team_id: String) -> Result<(), StorageError> {
    validate_team_id(Some(&team_id))?;
    let mut statement = cx
        .conn
        .prepare_cached("SELECT tab_id, generation FROM tab_todo_generations WHERE team_id = ?1")
        .map_err(|error| sqlite_error("prepare tab todo team cleanup", error))?;
    let rows = statement
        .query_map([&team_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|error| sqlite_error("read tab todo team cleanup", error))?;
    let mut generations = Vec::new();
    for row in rows {
        generations.push(row.map_err(|error| sqlite_error("decode tab todo team cleanup", error))?);
    }
    drop(statement);
    for (tab_id, generation) in generations {
        forget_generation(cx, tab_id, generation)?;
    }
    Ok(())
}

pub(crate) fn forget_workspace(
    cx: &mut WriteCx<'_>,
    workspace_id: String,
) -> Result<(), StorageError> {
    validate_workspace_id(&workspace_id)?;
    let mut statement = cx
        .conn
        .prepare_cached(
            "SELECT tab_id, generation FROM tab_todo_generations WHERE workspace_id = ?1",
        )
        .map_err(|error| sqlite_error("prepare tab todo workspace cleanup", error))?;
    let rows = statement
        .query_map([&workspace_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|error| sqlite_error("read tab todo workspace cleanup", error))?;
    let mut generations = Vec::new();
    for row in rows {
        generations
            .push(row.map_err(|error| sqlite_error("decode tab todo workspace cleanup", error))?);
    }
    drop(statement);
    for (tab_id, generation) in generations {
        forget_generation(cx, tab_id, generation)?;
    }
    Ok(())
}

fn forget_generation(
    cx: &mut WriteCx<'_>,
    tab_id: String,
    generation: i64,
) -> Result<(), StorageError> {
    cx.write(
        "DELETE FROM tab_todo_items WHERE tab_id = ?1 AND generation = ?2",
        params![tab_id, generation],
    )?;
    cx.write(
        "DELETE FROM tab_todo_audit WHERE tab_id = ?1 AND generation = ?2",
        params![tab_id, generation],
    )?;
    cx.write(
        "DELETE FROM tab_todo_generations WHERE tab_id = ?1 AND generation = ?2",
        params![tab_id, generation],
    )?;
    cx.write(
        "DELETE FROM tab_todo_heads WHERE tab_id = ?1 AND generation = ?2",
        params![tab_id, generation],
    )?;
    Ok(())
}

fn insert_generation(
    cx: &mut WriteCx<'_>,
    tab_id: &str,
    generation: u64,
    workspace_id: &str,
    team_id: Option<&str>,
    started_at: i64,
) -> Result<(), StorageError> {
    cx.write("INSERT INTO tab_todo_generations (tab_id, generation, workspace_id, team_id, status, started_at, revision) VALUES (?1, ?2, ?3, ?4, 'active', ?5, 1)", params![tab_id, to_sql_u64("todo generation", generation)?, workspace_id, team_id, started_at])?;
    Ok(())
}

fn insert_item(
    cx: &mut WriteCx<'_>,
    tab_id: &str,
    generation: u64,
    position: usize,
    item: &TabTodoNewItemRecord,
) -> Result<(), StorageError> {
    cx.write("INSERT INTO tab_todo_items (tab_id, generation, item_id, position, title, description, state, revision, deleted, link_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'open', 1, 0, ?7)", params![tab_id, to_sql_u64("todo generation", generation)?, item.id, position as i64, item.title, item.description, item.link_json])?;
    Ok(())
}

fn check_item_write(
    conn: &Connection,
    tab_id: &str,
    generation: i64,
    item_id: &str,
    expected: i64,
    changed: usize,
) -> Result<(), StorageError> {
    if changed == 1 {
        return Ok(());
    }
    let actual: Option<i64> = conn.prepare_cached("SELECT revision FROM tab_todo_items WHERE tab_id = ?1 AND generation = ?2 AND item_id = ?3 AND deleted = 0")
        .and_then(|mut statement| statement.query_row(params![tab_id, generation, item_id], |row| row.get(0)).optional())
        .map_err(|error| sqlite_error("check tab todo item revision", error))?;
    if let Some(actual) = actual {
        return Err(StorageError::StaleRevision {
            entity: format!("todo item {item_id}"),
            expected: Some(Revision(expected as u64)),
            actual: Some(Revision(actual as u64)),
        });
    }
    Err(StorageError::NotFound {
        entity: format!("todo item {item_id}"),
    })
}

fn current_generation(conn: &Connection, tab_id: &str) -> Result<Option<i64>, StorageError> {
    conn.prepare_cached("SELECT generation FROM tab_todo_heads WHERE tab_id = ?1")
        .and_then(|mut statement| statement.query_row([tab_id], |row| row.get(0)).optional())
        .map_err(|error| sqlite_error("read current tab todo generation", error))
}

fn require_current_generation(
    conn: &Connection,
    tab_id: &str,
    generation: i64,
) -> Result<(), StorageError> {
    let actual = current_generation(conn, tab_id)?;
    if actual == Some(generation) {
        Ok(())
    } else {
        Err(stale_generation(tab_id, Some(generation), actual))
    }
}

fn require_mutable_generation(
    conn: &Connection,
    tab_id: &str,
    generation: i64,
) -> Result<(), StorageError> {
    require_current_generation(conn, tab_id, generation)?;
    let status: Option<String> = conn
        .prepare_cached(
            "SELECT status FROM tab_todo_generations WHERE tab_id = ?1 AND generation = ?2",
        )
        .and_then(|mut statement| {
            statement
                .query_row(params![tab_id, generation], |row| row.get(0))
                .optional()
        })
        .map_err(|error| sqlite_error("read tab todo status", error))?;
    if matches!(status.as_deref(), Some("active" | "complete")) {
        Ok(())
    } else {
        Err(StorageError::NotFound {
            entity: format!("mutable todo checklist for Tab {tab_id}"),
        })
    }
}

fn stale_generation(tab_id: &str, expected: Option<i64>, actual: Option<i64>) -> StorageError {
    StorageError::StaleRevision {
        entity: format!("todo checklist generation for Tab {tab_id}"),
        expected: expected.map(|value| Revision(value as u64)),
        actual: actual.map(|value| Revision(value as u64)),
    }
}

fn live_item_count(
    conn: &Connection,
    tab_id: &str,
    generation: i64,
) -> Result<(usize, usize), StorageError> {
    let (count, max): (i64, Option<i64>) = conn.prepare_cached("SELECT COUNT(*), MAX(position) FROM tab_todo_items WHERE tab_id = ?1 AND generation = ?2 AND deleted = 0")
        .and_then(|mut statement| statement.query_row(params![tab_id, generation], |row| Ok((row.get(0)?, row.get(1)?))))
        .map_err(|error| sqlite_error("count tab todo items", error))?;
    let count = usize::try_from(count).map_err(|_| corrupt("negative todo item count"))?;
    let next = max
        .map(|value| usize::try_from(value + 1).map_err(|_| corrupt("invalid todo position")))
        .transpose()?
        .unwrap_or(0);
    Ok((count, next))
}

fn compact_live_positions(
    cx: &mut WriteCx<'_>,
    tab_id: &str,
    generation: i64,
) -> Result<(), StorageError> {
    let mut statement = cx.conn.prepare_cached("SELECT item_id FROM tab_todo_items WHERE tab_id = ?1 AND generation = ?2 AND deleted = 0 ORDER BY position")
        .map_err(|error| sqlite_error("prepare tab todo position compaction", error))?;
    let rows = statement
        .query_map(params![tab_id, generation], |row| row.get::<_, String>(0))
        .map_err(|error| sqlite_error("read tab todo position compaction", error))?;
    let mut item_ids = Vec::new();
    for row in rows {
        item_ids
            .push(row.map_err(|error| sqlite_error("decode tab todo position compaction", error))?);
    }
    drop(statement);
    for (position, item_id) in item_ids.into_iter().enumerate() {
        cx.write("UPDATE tab_todo_items SET position = ?4 WHERE tab_id = ?1 AND generation = ?2 AND item_id = ?3 AND deleted = 0", params![tab_id, generation, item_id, position as i64])?;
    }
    Ok(())
}

fn update_lifecycle(
    cx: &mut WriteCx<'_>,
    tab_id: &str,
    generation: i64,
) -> Result<(), StorageError> {
    let (items, unfinished): (i64, i64) = cx.conn.prepare_cached("SELECT COUNT(*), SUM(CASE WHEN state != 'done' THEN 1 ELSE 0 END) FROM tab_todo_items WHERE tab_id = ?1 AND generation = ?2 AND deleted = 0")
        .and_then(|mut statement| statement.query_row(params![tab_id, generation], |row| Ok((row.get(0)?, row.get::<_, Option<i64>>(1)?.unwrap_or(0)))) )
        .map_err(|error| sqlite_error("read tab todo completion", error))?;
    let status = if items == 0 {
        "cleared"
    } else if unfinished == 0 {
        "complete"
    } else {
        "active"
    };
    set_status(cx, tab_id, generation, status)
}

fn set_status(
    cx: &mut WriteCx<'_>,
    tab_id: &str,
    generation: i64,
    status: &str,
) -> Result<(), StorageError> {
    cx.write("UPDATE tab_todo_generations SET status = ?3, revision = revision + 1 WHERE tab_id = ?1 AND generation = ?2", params![tab_id, generation, status])?;
    Ok(())
}

fn touch_generation(
    cx: &mut WriteCx<'_>,
    tab_id: &str,
    generation: i64,
) -> Result<(), StorageError> {
    cx.write("UPDATE tab_todo_generations SET revision = revision + 1 WHERE tab_id = ?1 AND generation = ?2", params![tab_id, generation])?;
    Ok(())
}

fn audit(
    cx: &mut WriteCx<'_>,
    tab_id: &str,
    generation: i64,
    item_id: Option<&str>,
    actor_tab_id: Option<&str>,
    action: &str,
    item_revision: Option<u64>,
    happened_at: i64,
) -> Result<(), StorageError> {
    cx.write("INSERT INTO tab_todo_audit (tab_id, generation, item_id, actor_tab_id, action, item_revision, happened_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", params![tab_id, generation, item_id, actor_tab_id, action, item_revision.map(|value| to_sql_u64("todo item revision", value)).transpose()?, happened_at])?;
    cx.write("DELETE FROM tab_todo_audit WHERE tab_id = ?1 AND event_id NOT IN (SELECT event_id FROM tab_todo_audit WHERE tab_id = ?1 ORDER BY event_id DESC LIMIT 64)", [tab_id])?;
    Ok(())
}

fn validate_actor(actor: Option<&str>) -> Result<(), StorageError> {
    actor.map(validate_tab_id).unwrap_or(Ok(()))
}
fn validate_state(state: &str) -> Result<(), StorageError> {
    if matches!(state, "open" | "in_progress" | "done") {
        Ok(())
    } else {
        Err(StorageError::invalid("todo state", "unknown state"))
    }
}
fn corrupt(detail: &'static str) -> StorageError {
    StorageError::Corrupt {
        detail: detail.into(),
    }
}
