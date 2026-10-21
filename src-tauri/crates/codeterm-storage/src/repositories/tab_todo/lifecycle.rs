//! Item state transitions with attempt accounting and the bounded per-item event log.

use super::mutations::{
    audit, corrupt, require_mutable_generation, touch_generation, update_lifecycle, validate_actor,
};
use super::queries;
use super::records::{
    validate_item_id, validate_reason, validate_tab_id, validate_team_id, validate_workspace_id,
    TabTodoChecklistRecord, MAX_TAB_TODO_ITEM_EVENTS,
};
use crate::error::{sqlite_error, StorageError};
use crate::owner::WriteCx;
use crate::repositories::{to_sql_u64, Revision};
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ItemProgress {
    pub state: String,
    pub attempt: i64,
    pub attempt_closed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Transition {
    pub event: Option<&'static str>,
    pub attempt: i64,
    pub attempt_closed: bool,
}

/// A start after a finished attempt (done, skipped or reopened) is a restart:
/// the same item opens the next attempt instead of a new item.
pub(crate) fn transition(current: &ItemProgress, target: &str) -> Transition {
    let same = current.state == target;
    let mut next = Transition {
        event: None,
        attempt: current.attempt,
        attempt_closed: current.attempt_closed,
    };
    match target {
        "in_progress" if !same => {
            next.event = Some("started");
            if current.attempt == 0 || current.attempt_closed {
                next.attempt = current.attempt + 1;
                next.attempt_closed = false;
            }
        }
        "done" if !same => {
            next.event = Some("completed");
            next.attempt = current.attempt.max(1);
            next.attempt_closed = true;
        }
        "open" if matches!(current.state.as_str(), "done" | "skipped") => {
            next.event = Some("reopened");
        }
        "blocked" => next.event = Some("blocked"),
        "skipped" => {
            next.event = Some("skipped");
            next.attempt_closed = true;
        }
        _ => {}
    }
    next
}

pub(crate) fn reopen_transition(current: &ItemProgress) -> Transition {
    Transition {
        event: Some("reopened"),
        attempt: current.attempt,
        attempt_closed: true,
    }
}

pub(crate) fn set_state(
    cx: &mut WriteCx<'_>,
    tab_id: String,
    generation: u64,
    item_id: String,
    expected_revision: u64,
    state: String,
    reason: Option<String>,
    actor_tab_id: Option<String>,
    at: u64,
) -> Result<TabTodoChecklistRecord, StorageError> {
    validate_tab_id(&tab_id)?;
    validate_item_id(&item_id)?;
    validate_actor(actor_tab_id.as_deref())?;
    validate_state(&state)?;
    validate_reason(
        reason.as_deref(),
        matches!(state.as_str(), "blocked" | "skipped"),
    )?;
    let generation = to_sql_u64("todo generation", generation)?;
    let expected_revision = to_sql_u64("todo item revision", expected_revision)?;
    let at = to_sql_u64("todo audit time", at)?;
    require_mutable_generation(cx.conn, &tab_id, generation)?;
    let (revision, progress) = read_progress(cx.conn, &tab_id, generation, &item_id)?;
    if revision != expected_revision {
        return Err(StorageError::StaleRevision {
            entity: format!("todo item {item_id}"),
            expected: Some(Revision(expected_revision as u64)),
            actual: Some(Revision(revision as u64)),
        });
    }
    let next = transition(&progress, &state);
    let step = Step {
        tab_id: &tab_id,
        generation,
        item_id: &item_id,
        state: &state,
        reason: reason.as_deref(),
        actor_tab_id: actor_tab_id.as_deref(),
        at,
    };
    apply(cx, &step, revision, &next)?;
    update_lifecycle(cx, &tab_id, generation)?;
    touch_generation(cx, &tab_id, generation)?;
    audit(
        cx,
        &tab_id,
        generation,
        Some(&item_id),
        actor_tab_id.as_deref(),
        "state_change",
        Some(revision as u64 + 1),
        at,
    )?;
    queries::load(cx.conn, &tab_id)?.ok_or_else(|| corrupt("updated tab todo missing"))
}

/// Reviewer reopen: the named item returns to pending with the review reason,
/// and the rejected attempt is closed so the next start counts as a restart.
pub(crate) fn reopen(
    cx: &mut WriteCx<'_>,
    tab_id: String,
    generation: u64,
    item_id: String,
    reason: String,
    actor_tab_id: Option<String>,
    at: u64,
) -> Result<TabTodoChecklistRecord, StorageError> {
    validate_tab_id(&tab_id)?;
    validate_item_id(&item_id)?;
    validate_actor(actor_tab_id.as_deref())?;
    validate_reason(Some(&reason), true)?;
    let generation = to_sql_u64("todo generation", generation)?;
    let at = to_sql_u64("todo audit time", at)?;
    require_mutable_generation(cx.conn, &tab_id, generation)?;
    let (revision, progress) = read_progress(cx.conn, &tab_id, generation, &item_id)?;
    let step = Step {
        tab_id: &tab_id,
        generation,
        item_id: &item_id,
        state: "open",
        reason: Some(&reason),
        actor_tab_id: actor_tab_id.as_deref(),
        at,
    };
    apply(cx, &step, revision, &reopen_transition(&progress))?;
    update_lifecycle(cx, &tab_id, generation)?;
    touch_generation(cx, &tab_id, generation)?;
    audit(
        cx,
        &tab_id,
        generation,
        Some(&item_id),
        actor_tab_id.as_deref(),
        "reopen",
        Some(revision as u64 + 1),
        at,
    )?;
    queries::load(cx.conn, &tab_id)?.ok_or_else(|| corrupt("reopened tab todo missing"))
}

/// Moves every generation, item, event and audit row of `source_tab_id` to a
/// replacement tab that has no checklist yet. Returns `None` without a source list.
pub(crate) fn transfer(
    cx: &mut WriteCx<'_>,
    source_tab_id: String,
    target_tab_id: String,
    workspace_id: String,
    team_id: Option<String>,
    actor_tab_id: Option<String>,
    at: u64,
) -> Result<Option<TabTodoChecklistRecord>, StorageError> {
    validate_tab_id(&source_tab_id)?;
    validate_tab_id(&target_tab_id)?;
    validate_workspace_id(&workspace_id)?;
    validate_team_id(team_id.as_deref())?;
    validate_actor(actor_tab_id.as_deref())?;
    if source_tab_id == target_tab_id {
        return Err(StorageError::invalid(
            "todo carry-over",
            "source and target tabs are the same",
        ));
    }
    let at = to_sql_u64("todo audit time", at)?;
    if head_generation(cx.conn, &target_tab_id)?.is_some() {
        return Err(StorageError::invalid(
            "todo carry-over",
            "target tab already has a checklist",
        ));
    }
    let Some(generation) = head_generation(cx.conn, &source_tab_id)? else {
        return Ok(None);
    };
    cx.write("INSERT INTO tab_todo_generations (tab_id, generation, workspace_id, team_id, status, started_at, revision, proposal_json) SELECT ?2, generation, ?3, ?4, status, started_at, revision + 1, proposal_json FROM tab_todo_generations WHERE tab_id = ?1", params![source_tab_id, target_tab_id, workspace_id, team_id])?;
    cx.write(
        "UPDATE tab_todo_items SET tab_id = ?2 WHERE tab_id = ?1",
        params![source_tab_id, target_tab_id],
    )?;
    cx.write(
        "DELETE FROM tab_todo_generations WHERE tab_id = ?1",
        [&source_tab_id],
    )?;
    cx.write(
        "UPDATE tab_todo_item_events SET tab_id = ?2 WHERE tab_id = ?1",
        params![source_tab_id, target_tab_id],
    )?;
    cx.write(
        "UPDATE tab_todo_audit SET tab_id = ?2 WHERE tab_id = ?1",
        params![source_tab_id, target_tab_id],
    )?;
    cx.write(
        "UPDATE tab_todo_heads SET tab_id = ?2 WHERE tab_id = ?1",
        params![source_tab_id, target_tab_id],
    )?;
    audit(
        cx,
        &target_tab_id,
        generation,
        None,
        actor_tab_id.as_deref(),
        "carry_over",
        None,
        at,
    )?;
    queries::load(cx.conn, &target_tab_id)?
        .map(Some)
        .ok_or_else(|| corrupt("carried tab todo missing"))
}

struct Step<'a> {
    tab_id: &'a str,
    generation: i64,
    item_id: &'a str,
    state: &'a str,
    reason: Option<&'a str>,
    actor_tab_id: Option<&'a str>,
    at: i64,
}

fn apply(
    cx: &mut WriteCx<'_>,
    step: &Step<'_>,
    revision: i64,
    next: &Transition,
) -> Result<(), StorageError> {
    let counter = next.event.map(counter_column).unwrap_or("folded_events");
    let increment = i64::from(next.event.is_some());
    let sql = format!("UPDATE tab_todo_items SET state = ?5, reason = ?6, attempt = ?7, attempt_closed = ?8, {counter} = {counter} + ?9, revision = revision + 1 WHERE tab_id = ?1 AND generation = ?2 AND item_id = ?3 AND revision = ?4 AND deleted = 0");
    let changed = cx.write(
        &sql,
        params![
            step.tab_id,
            step.generation,
            step.item_id,
            revision,
            step.state,
            step.reason,
            next.attempt,
            next.attempt_closed,
            increment
        ],
    )?;
    if changed != 1 {
        return Err(corrupt("tab todo item changed inside its write"));
    }
    if let Some(kind) = next.event {
        record_event(cx, step, kind, next.attempt)?;
    }
    Ok(())
}

fn record_event(
    cx: &mut WriteCx<'_>,
    step: &Step<'_>,
    kind: &str,
    attempt: i64,
) -> Result<(), StorageError> {
    cx.write("INSERT INTO tab_todo_item_events (tab_id, generation, item_id, kind, attempt, actor_tab_id, reason, happened_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)", params![step.tab_id, step.generation, step.item_id, kind, attempt, step.actor_tab_id, step.reason, step.at])?;
    let folded = cx.write(&format!("DELETE FROM tab_todo_item_events WHERE tab_id = ?1 AND generation = ?2 AND item_id = ?3 AND event_id NOT IN (SELECT event_id FROM tab_todo_item_events WHERE tab_id = ?1 AND generation = ?2 AND item_id = ?3 ORDER BY event_id DESC LIMIT {MAX_TAB_TODO_ITEM_EVENTS})"), params![step.tab_id, step.generation, step.item_id])?;
    if folded > 0 {
        cx.write("UPDATE tab_todo_items SET folded_events = folded_events + ?4 WHERE tab_id = ?1 AND generation = ?2 AND item_id = ?3", params![step.tab_id, step.generation, step.item_id, folded as i64])?;
    }
    Ok(())
}

fn counter_column(kind: &str) -> &'static str {
    match kind {
        "started" => "started_count",
        "completed" => "completed_count",
        "reopened" => "reopened_count",
        "blocked" => "blocked_count",
        _ => "skipped_count",
    }
}

fn read_progress(
    conn: &Connection,
    tab_id: &str,
    generation: i64,
    item_id: &str,
) -> Result<(i64, ItemProgress), StorageError> {
    conn.prepare_cached("SELECT revision, state, attempt, attempt_closed FROM tab_todo_items WHERE tab_id = ?1 AND generation = ?2 AND item_id = ?3 AND deleted = 0")
        .and_then(|mut statement| {
            statement
                .query_row(params![tab_id, generation, item_id], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        ItemProgress {
                            state: row.get(1)?,
                            attempt: row.get(2)?,
                            attempt_closed: row.get(3)?,
                        },
                    ))
                })
                .optional()
        })
        .map_err(|error| sqlite_error("read tab todo item progress", error))?
        .ok_or_else(|| StorageError::NotFound {
            entity: format!("todo item {item_id}"),
        })
}

fn head_generation(conn: &Connection, tab_id: &str) -> Result<Option<i64>, StorageError> {
    conn.prepare_cached("SELECT generation FROM tab_todo_heads WHERE tab_id = ?1")
        .and_then(|mut statement| statement.query_row([tab_id], |row| row.get(0)).optional())
        .map_err(|error| sqlite_error("read tab todo head", error))
}

fn validate_state(state: &str) -> Result<(), StorageError> {
    if matches!(
        state,
        "open" | "in_progress" | "done" | "blocked" | "skipped"
    ) {
        Ok(())
    } else {
        Err(StorageError::invalid("todo state", "unknown state"))
    }
}
