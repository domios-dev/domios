use super::store_rows::{
    join_plan, join_plan_level, AttemptRow, KeyedRow, PlanRows, StoredPlanLevel, StoredTask,
    TaskRows,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde_json::Value;
use std::collections::HashMap;

// Query columns are VIRTUAL generated columns over `row_json`, so they never drift from the row.
pub(super) const TASK_SCHEMA: &str = r#"
    CREATE TABLE IF NOT EXISTS plan_tasks (
        plan_id TEXT NOT NULL,
        task_id TEXT NOT NULL,
        position INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        content_hash INTEGER NOT NULL,
        milestone_count INTEGER NOT NULL,
        milestone_digest INTEGER NOT NULL,
        row_json TEXT NOT NULL,
        status TEXT GENERATED ALWAYS AS (json_extract(row_json, '$.status')) VIRTUAL,
        assigned_tab_id TEXT GENERATED ALWAYS AS (json_extract(row_json, '$.assigned_tab_id')) VIRTUAL,
        lane_worktree_path TEXT GENERATED ALWAYS AS (json_extract(row_json, '$.lane.worktree_path')) VIRTUAL,
        lane_branch TEXT GENERATED ALWAYS AS (json_extract(row_json, '$.lane.branch')) VIRTUAL,
        PRIMARY KEY (plan_id, task_id),
        FOREIGN KEY (plan_id) REFERENCES plans(id) ON DELETE CASCADE
    );
    CREATE INDEX IF NOT EXISTS plan_tasks_status ON plan_tasks(status);
    CREATE INDEX IF NOT EXISTS plan_tasks_assigned_tab_id
        ON plan_tasks(assigned_tab_id) WHERE assigned_tab_id IS NOT NULL;
    CREATE INDEX IF NOT EXISTS plan_tasks_lane_worktree_path
        ON plan_tasks(lane_worktree_path) WHERE lane_worktree_path IS NOT NULL;
    CREATE INDEX IF NOT EXISTS plan_tasks_lane_branch
        ON plan_tasks(lane_branch) WHERE lane_branch IS NOT NULL;
    CREATE TABLE IF NOT EXISTS plan_task_milestones (
        plan_id TEXT NOT NULL,
        task_id TEXT NOT NULL,
        seq INTEGER NOT NULL,
        row_json TEXT NOT NULL,
        PRIMARY KEY (plan_id, task_id, seq),
        FOREIGN KEY (plan_id, task_id) REFERENCES plan_tasks(plan_id, task_id) ON DELETE CASCADE
    );
    CREATE TABLE IF NOT EXISTS plan_task_attempts (
        plan_id TEXT NOT NULL,
        task_id TEXT NOT NULL,
        attempt_id TEXT NOT NULL,
        position INTEGER NOT NULL,
        content_hash INTEGER NOT NULL,
        row_json TEXT NOT NULL,
        executor_tab_id TEXT GENERATED ALWAYS AS (json_extract(row_json, '$.executor_tab_id')) VIRTUAL,
        session_id TEXT GENERATED ALWAYS AS (json_extract(row_json, '$.session_id')) VIRTUAL,
        session_binding_nonce TEXT GENERATED ALWAYS AS (json_extract(row_json, '$.session_binding_nonce')) VIRTUAL,
        open INTEGER GENERATED ALWAYS AS (
            json_extract(row_json, '$.closed_at') IS NULL
            AND json_extract(row_json, '$.retired_at') IS NULL
        ) VIRTUAL,
        PRIMARY KEY (plan_id, task_id, position),
        FOREIGN KEY (plan_id, task_id) REFERENCES plan_tasks(plan_id, task_id) ON DELETE CASCADE
    );
    CREATE INDEX IF NOT EXISTS plan_task_attempts_executor_tab_id
        ON plan_task_attempts(executor_tab_id, open);
    CREATE INDEX IF NOT EXISTS plan_task_attempts_session_id
        ON plan_task_attempts(session_id) WHERE session_id IS NOT NULL;
    CREATE INDEX IF NOT EXISTS plan_task_attempts_session_binding_nonce
        ON plan_task_attempts(session_binding_nonce) WHERE session_binding_nonce IS NOT NULL;
    CREATE INDEX IF NOT EXISTS plan_task_attempts_open
        ON plan_task_attempts(open) WHERE open;
"#;

pub(super) const PLAN_LEVEL_SCHEMA: &str = r#"
    CREATE TABLE IF NOT EXISTS plan_sections (
        plan_id TEXT NOT NULL,
        section_id TEXT NOT NULL,
        position INTEGER NOT NULL,
        content_hash INTEGER NOT NULL,
        row_json TEXT NOT NULL,
        PRIMARY KEY (plan_id, section_id),
        FOREIGN KEY (plan_id) REFERENCES plans(id) ON DELETE CASCADE
    );
    CREATE TABLE IF NOT EXISTS plan_elements (
        plan_id TEXT NOT NULL,
        element_id TEXT NOT NULL,
        position INTEGER NOT NULL,
        content_hash INTEGER NOT NULL,
        row_json TEXT NOT NULL,
        PRIMARY KEY (plan_id, element_id),
        FOREIGN KEY (plan_id) REFERENCES plans(id) ON DELETE CASCADE
    );
    CREATE TABLE IF NOT EXISTS plan_strides (
        plan_id TEXT NOT NULL,
        seq INTEGER NOT NULL,
        row_json TEXT NOT NULL,
        PRIMARY KEY (plan_id, seq),
        FOREIGN KEY (plan_id) REFERENCES plans(id) ON DELETE CASCADE
    );
    CREATE TABLE IF NOT EXISTS plan_heartbeat (
        plan_id TEXT PRIMARY KEY,
        content_hash INTEGER NOT NULL,
        row_json TEXT NOT NULL,
        FOREIGN KEY (plan_id) REFERENCES plans(id) ON DELETE CASCADE
    );
"#;

/// Rows one write touched; `changed_tasks` scopes the incremental search re-index.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct RowWrites {
    pub(crate) tasks: usize,
    pub(crate) milestones: usize,
    pub(crate) attempts: usize,
    pub(crate) sections: usize,
    pub(crate) elements: usize,
    pub(crate) strides: usize,
    pub(crate) heartbeat: usize,
    pub(crate) deleted: usize,
    pub(crate) changed_tasks: Vec<String>,
    pub(crate) header_identity_changed: bool,
    /// The `(project_path, canonical_project_path)` this write moved the plan away from.
    pub(crate) previous_project_paths: Option<(String, String)>,
}

struct StoredTaskState {
    position: i64,
    hash: i64,
    milestone_count: usize,
    milestone_digest: i64,
}

pub(super) fn sync_task_rows(
    transaction: &Transaction<'_>,
    plan_id: &str,
    tasks: &[TaskRows],
) -> rusqlite::Result<RowWrites> {
    let mut stored = stored_tasks(transaction, plan_id)?;
    let mut stored_attempts = stored_attempts(transaction, plan_id)?;
    let mut writes = RowWrites::default();
    for (position, task) in tasks.iter().enumerate() {
        let position = position as i64;
        let changed = match stored.remove(&task.id) {
            None => {
                insert_task(transaction, plan_id, position, task)?;
                writes.tasks += 1;
                writes.milestones += task.milestones.len();
                for (attempt_position, attempt) in task.attempts.iter().enumerate() {
                    insert_attempt(transaction, plan_id, &task.id, attempt_position, attempt)?;
                }
                writes.attempts += task.attempts.len();
                true
            }
            Some(state) => {
                let document_changed = state.hash != task.hash;
                if document_changed || state.position != position {
                    transaction.execute(
                        "UPDATE plan_tasks SET position = ?3, updated_at = ?4, content_hash = ?5, row_json = ?6
                         WHERE plan_id = ?1 AND task_id = ?2",
                        params![plan_id, task.id, position, task.updated_at, task.hash, task.document],
                    )?;
                    writes.tasks += usize::from(document_changed);
                }
                let milestones_changed =
                    sync_milestones(transaction, plan_id, task, &state, &mut writes)?;
                let attempts_changed = sync_attempts(
                    transaction,
                    plan_id,
                    task,
                    &mut stored_attempts,
                    &mut writes,
                )?;
                document_changed || milestones_changed || attempts_changed
            }
        };
        if changed {
            writes.changed_tasks.push(task.id.clone());
        }
    }
    for (task_id, position) in stored_attempts.into_keys() {
        if stored.contains_key(&task_id) {
            continue;
        }
        transaction.execute(
            "DELETE FROM plan_task_attempts WHERE plan_id = ?1 AND task_id = ?2 AND position = ?3",
            params![plan_id, task_id, position],
        )?;
        writes.deleted += 1;
        if !writes.changed_tasks.contains(&task_id) {
            writes.changed_tasks.push(task_id);
        }
    }
    let mut removed: Vec<String> = stored.into_keys().collect();
    removed.sort();
    for task_id in &removed {
        transaction.execute(
            "DELETE FROM plan_tasks WHERE plan_id = ?1 AND task_id = ?2",
            params![plan_id, task_id],
        )?;
        writes.deleted += 1;
    }

    Ok(writes)
}

fn insert_task(
    transaction: &Transaction<'_>,
    plan_id: &str,
    position: i64,
    task: &TaskRows,
) -> rusqlite::Result<()> {
    transaction.execute(
        "INSERT INTO plan_tasks(plan_id, task_id, position, updated_at, content_hash, milestone_count, milestone_digest, row_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            plan_id,
            task.id,
            position,
            task.updated_at,
            task.hash,
            task.milestones.len() as i64,
            task.milestone_digest(),
            task.document
        ],
    )?;
    insert_milestones(transaction, plan_id, task, 0)
}

fn insert_milestones(
    transaction: &Transaction<'_>,
    plan_id: &str,
    task: &TaskRows,
    from: usize,
) -> rusqlite::Result<()> {
    let mut statement = transaction.prepare_cached(
        "INSERT INTO plan_task_milestones(plan_id, task_id, seq, row_json) VALUES (?1, ?2, ?3, ?4)",
    )?;
    for (seq, milestone) in task.milestones.iter().enumerate().skip(from) {
        statement.execute(params![plan_id, task.id, seq as i64, milestone])?;
    }
    Ok(())
}

/// Appends new milestones when the stored ones are an unchanged prefix, otherwise rewrites them.
fn sync_milestones(
    transaction: &Transaction<'_>,
    plan_id: &str,
    task: &TaskRows,
    state: &StoredTaskState,
    writes: &mut RowWrites,
) -> rusqlite::Result<bool> {
    let count = task.milestones.len();
    if state.milestone_count == count && state.milestone_digest == task.milestone_digest() {
        return Ok(false);
    }
    let prefix_digest = match state.milestone_count {
        0 => Some(0),
        stored => task.milestone_digests.get(stored - 1).copied(),
    };
    let from = if prefix_digest == Some(state.milestone_digest) {
        state.milestone_count
    } else {
        writes.deleted += transaction.execute(
            "DELETE FROM plan_task_milestones WHERE plan_id = ?1 AND task_id = ?2",
            params![plan_id, task.id],
        )?;
        0
    };
    insert_milestones(transaction, plan_id, task, from)?;
    writes.milestones += count - from;
    transaction.execute(
        "UPDATE plan_tasks SET milestone_count = ?3, milestone_digest = ?4 WHERE plan_id = ?1 AND task_id = ?2",
        params![plan_id, task.id, count as i64, task.milestone_digest()],
    )?;
    Ok(true)
}

fn sync_attempts(
    transaction: &Transaction<'_>,
    plan_id: &str,
    task: &TaskRows,
    stored: &mut HashMap<(String, i64), i64>,
    writes: &mut RowWrites,
) -> rusqlite::Result<bool> {
    let mut changed = false;
    for (position, attempt) in task.attempts.iter().enumerate() {
        match stored.remove(&(task.id.clone(), position as i64)) {
            None => insert_attempt(transaction, plan_id, &task.id, position, attempt)?,
            Some(hash) if hash == attempt.hash => continue,
            Some(_) => {
                transaction.execute(
                    "UPDATE plan_task_attempts SET attempt_id = ?4, content_hash = ?5, row_json = ?6
                     WHERE plan_id = ?1 AND task_id = ?2 AND position = ?3",
                    params![plan_id, task.id, position as i64, attempt.id, attempt.hash, attempt.document],
                )?;
            }
        }
        writes.attempts += 1;
        changed = true;
    }
    Ok(changed)
}

fn insert_attempt(
    transaction: &Transaction<'_>,
    plan_id: &str,
    task_id: &str,
    position: usize,
    attempt: &AttemptRow,
) -> rusqlite::Result<()> {
    transaction
        .prepare_cached(
            "INSERT INTO plan_task_attempts(plan_id, task_id, attempt_id, position, content_hash, row_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?
        .execute(params![
            plan_id,
            task_id,
            attempt.id,
            position as i64,
            attempt.hash,
            attempt.document
        ])?;
    Ok(())
}

fn stored_tasks(
    connection: &Connection,
    plan_id: &str,
) -> rusqlite::Result<HashMap<String, StoredTaskState>> {
    let mut statement = connection.prepare_cached(
        "SELECT task_id, position, content_hash, milestone_count, milestone_digest FROM plan_tasks WHERE plan_id = ?1",
    )?;
    let rows = statement.query_map([plan_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            StoredTaskState {
                position: row.get(1)?,
                hash: row.get(2)?,
                milestone_count: row.get::<_, i64>(3)? as usize,
                milestone_digest: row.get(4)?,
            },
        ))
    })?;
    rows.collect()
}

fn stored_attempts(
    connection: &Connection,
    plan_id: &str,
) -> rusqlite::Result<HashMap<(String, i64), i64>> {
    let mut statement = connection.prepare_cached(
        "SELECT task_id, position, content_hash FROM plan_task_attempts WHERE plan_id = ?1",
    )?;
    let rows = statement.query_map([plan_id], |row| {
        Ok(((row.get::<_, String>(0)?, row.get(1)?), row.get(2)?))
    })?;
    rows.collect()
}

/// The stride prefix a plan row records, read before its header is rewritten.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct StoredStrides {
    pub(super) count: usize,
    pub(super) digest: i64,
}

struct KeyedTable {
    table: &'static str,
    key: &'static str,
}

const SECTIONS: KeyedTable = KeyedTable {
    table: "plan_sections",
    key: "section_id",
};
const ELEMENTS: KeyedTable = KeyedTable {
    table: "plan_elements",
    key: "element_id",
};

pub(super) fn sync_plan_level_rows(
    transaction: &Transaction<'_>,
    plan_id: &str,
    rows: &PlanRows,
    stored_strides: StoredStrides,
    writes: &mut RowWrites,
) -> rusqlite::Result<()> {
    writes.sections += sync_keyed_rows(
        transaction,
        &SECTIONS,
        plan_id,
        &rows.sections,
        &mut writes.deleted,
    )?;
    writes.elements += sync_keyed_rows(
        transaction,
        &ELEMENTS,
        plan_id,
        &rows.elements,
        &mut writes.deleted,
    )?;
    writes.strides += sync_strides(
        transaction,
        plan_id,
        rows,
        stored_strides,
        &mut writes.deleted,
    )?;
    writes.heartbeat += sync_heartbeat(
        transaction,
        plan_id,
        rows.heartbeat.as_deref(),
        &mut writes.deleted,
    )?;
    Ok(())
}

/// Rewrites only changed rows, moves reordered ones, and deletes removed ones.
fn sync_keyed_rows(
    transaction: &Transaction<'_>,
    table: &KeyedTable,
    plan_id: &str,
    rows: &[KeyedRow],
    deleted: &mut usize,
) -> rusqlite::Result<usize> {
    let KeyedTable { table, key } = table;
    let mut stored: HashMap<String, (i64, i64)> = {
        let mut statement = transaction.prepare_cached(&format!(
            "SELECT {key}, position, content_hash FROM {table} WHERE plan_id = ?1"
        ))?;
        let stored = statement.query_map([plan_id], |row| {
            Ok((row.get(0)?, (row.get(1)?, row.get(2)?)))
        })?;
        stored.collect::<rusqlite::Result<_>>()?
    };
    let mut written = 0;
    for (position, row) in rows.iter().enumerate() {
        let position = position as i64;
        match stored.remove(&row.id) {
            None => {
                transaction
                    .prepare_cached(&format!(
                        "INSERT INTO {table}(plan_id, {key}, position, content_hash, row_json)
                         VALUES (?1, ?2, ?3, ?4, ?5)"
                    ))?
                    .execute(params![plan_id, row.id, position, row.hash, row.document])?;
                written += 1;
            }
            Some((_, hash)) if hash != row.hash => {
                transaction
                    .prepare_cached(&format!(
                        "UPDATE {table} SET position = ?3, content_hash = ?4, row_json = ?5
                         WHERE plan_id = ?1 AND {key} = ?2"
                    ))?
                    .execute(params![plan_id, row.id, position, row.hash, row.document])?;
                written += 1;
            }
            Some((stored_position, _)) if stored_position != position => {
                transaction
                    .prepare_cached(&format!(
                        "UPDATE {table} SET position = ?3 WHERE plan_id = ?1 AND {key} = ?2"
                    ))?
                    .execute(params![plan_id, row.id, position])?;
            }
            Some(_) => {}
        }
    }
    let mut removed: Vec<String> = stored.into_keys().collect();
    removed.sort();
    for id in removed {
        *deleted += transaction.execute(
            &format!("DELETE FROM {table} WHERE plan_id = ?1 AND {key} = ?2"),
            params![plan_id, id],
        )?;
    }
    Ok(written)
}

/// Appends new strides when the stored ones are an unchanged prefix, otherwise rewrites them.
fn sync_strides(
    transaction: &Transaction<'_>,
    plan_id: &str,
    rows: &PlanRows,
    stored: StoredStrides,
    deleted: &mut usize,
) -> rusqlite::Result<usize> {
    let count = rows.strides.len();
    if stored.count == count && stored.digest == rows.stride_digest() {
        return Ok(0);
    }
    let prefix_digest = match stored.count {
        0 => Some(0),
        stored => rows.stride_digests.get(stored - 1).copied(),
    };
    let from = if prefix_digest == Some(stored.digest) {
        stored.count
    } else {
        *deleted +=
            transaction.execute("DELETE FROM plan_strides WHERE plan_id = ?1", [plan_id])?;
        0
    };
    let mut statement = transaction
        .prepare_cached("INSERT INTO plan_strides(plan_id, seq, row_json) VALUES (?1, ?2, ?3)")?;
    for (seq, stride) in rows.strides.iter().enumerate().skip(from) {
        statement.execute(params![plan_id, seq as i64, stride])?;
    }
    Ok(count - from)
}

fn sync_heartbeat(
    transaction: &Transaction<'_>,
    plan_id: &str,
    heartbeat: Option<&str>,
    deleted: &mut usize,
) -> rusqlite::Result<usize> {
    let stored: Option<i64> = transaction
        .query_row(
            "SELECT content_hash FROM plan_heartbeat WHERE plan_id = ?1",
            [plan_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(document) = heartbeat else {
        if stored.is_some() {
            *deleted +=
                transaction.execute("DELETE FROM plan_heartbeat WHERE plan_id = ?1", [plan_id])?;
        }
        return Ok(0);
    };
    let hash = super::store_rows::row_hash(document);
    if stored == Some(hash) {
        return Ok(0);
    }
    transaction.execute(
        "INSERT INTO plan_heartbeat(plan_id, content_hash, row_json) VALUES (?1, ?2, ?3)
         ON CONFLICT(plan_id) DO UPDATE SET content_hash = excluded.content_hash, row_json = excluded.row_json",
        params![plan_id, hash, document],
    )?;
    Ok(1)
}

/// Joins a plan header with its task and plan-level rows into the full plan document.
pub(super) fn load_plan_rows(
    connection: &Connection,
    plan_id: &str,
    header: &str,
) -> Result<Value, String> {
    let plan = parse(header)?;
    let mut milestones = grouped_documents(
        connection,
        "SELECT task_id, row_json FROM plan_task_milestones WHERE plan_id = ?1 ORDER BY task_id, seq",
        plan_id,
    )?;
    let mut attempts = grouped_documents(
        connection,
        "SELECT task_id, row_json FROM plan_task_attempts WHERE plan_id = ?1 ORDER BY task_id, position",
        plan_id,
    )?;
    let mut statement = connection
        .prepare_cached(
            "SELECT task_id, row_json FROM plan_tasks WHERE plan_id = ?1 ORDER BY position",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([plan_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?;
    let mut tasks = Vec::new();
    for row in rows {
        let (task_id, document) = row.map_err(|error| error.to_string())?;
        tasks.push(StoredTask {
            document: parse(&document)?,
            milestones: milestones.remove(&task_id).unwrap_or_default(),
            attempts: attempts.remove(&task_id).unwrap_or_default(),
        });
    }
    let plan = join_plan(plan, tasks)?;
    let level = StoredPlanLevel {
        sections: ordered_documents(
            connection,
            "SELECT row_json FROM plan_sections WHERE plan_id = ?1 ORDER BY position",
            plan_id,
        )?,
        elements: ordered_documents(
            connection,
            "SELECT row_json FROM plan_elements WHERE plan_id = ?1 ORDER BY position",
            plan_id,
        )?,
        strides: ordered_documents(
            connection,
            "SELECT row_json FROM plan_strides WHERE plan_id = ?1 ORDER BY seq",
            plan_id,
        )?,
        heartbeat: ordered_documents(
            connection,
            "SELECT row_json FROM plan_heartbeat WHERE plan_id = ?1",
            plan_id,
        )?
        .pop(),
    };
    join_plan_level(plan, level)
}

fn ordered_documents(
    connection: &Connection,
    sql: &str,
    plan_id: &str,
) -> Result<Vec<Value>, String> {
    let mut statement = connection
        .prepare_cached(sql)
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([plan_id], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?;
    rows.map(|row| parse(&row.map_err(|error| error.to_string())?))
        .collect()
}

pub(super) fn grouped_documents(
    connection: &Connection,
    sql: &str,
    plan_id: &str,
) -> Result<HashMap<String, Vec<Value>>, String> {
    let mut statement = connection
        .prepare_cached(sql)
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([plan_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?;
    let mut grouped: HashMap<String, Vec<Value>> = HashMap::new();
    for row in rows {
        let (task_id, document) = row.map_err(|error| error.to_string())?;
        grouped.entry(task_id).or_default().push(parse(&document)?);
    }
    Ok(grouped)
}

pub(super) fn parse(document: &str) -> Result<Value, String> {
    serde_json::from_str(document).map_err(|error| error.to_string())
}
