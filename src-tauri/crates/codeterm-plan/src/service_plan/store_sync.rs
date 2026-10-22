use super::store_rows::{join_plan, AttemptRow, StoredTask, TaskRows};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde_json::Value;
use std::collections::HashMap;

pub(super) const TASK_SCHEMA: &str = r#"
    CREATE TABLE IF NOT EXISTS plan_tasks (
        plan_id TEXT NOT NULL,
        task_id TEXT NOT NULL,
        position INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        content_hash INTEGER NOT NULL,
        milestone_count INTEGER NOT NULL,
        milestone_digest INTEGER NOT NULL,
        document_json TEXT NOT NULL,
        PRIMARY KEY (plan_id, task_id),
        FOREIGN KEY (plan_id) REFERENCES plans(id) ON DELETE CASCADE
    );
    CREATE TABLE IF NOT EXISTS plan_task_milestones (
        plan_id TEXT NOT NULL,
        task_id TEXT NOT NULL,
        seq INTEGER NOT NULL,
        document_json TEXT NOT NULL,
        PRIMARY KEY (plan_id, task_id, seq),
        FOREIGN KEY (plan_id, task_id) REFERENCES plan_tasks(plan_id, task_id) ON DELETE CASCADE
    );
    CREATE TABLE IF NOT EXISTS plan_task_attempts (
        plan_id TEXT NOT NULL,
        task_id TEXT NOT NULL,
        attempt_id TEXT NOT NULL,
        position INTEGER NOT NULL,
        content_hash INTEGER NOT NULL,
        document_json TEXT NOT NULL,
        PRIMARY KEY (plan_id, task_id, position),
        FOREIGN KEY (plan_id, task_id) REFERENCES plan_tasks(plan_id, task_id) ON DELETE CASCADE
    );
"#;

/// Rows one write touched; `changed_tasks` scopes the incremental search re-index.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct RowWrites {
    pub(crate) tasks: usize,
    pub(crate) milestones: usize,
    pub(crate) attempts: usize,
    pub(crate) deleted: usize,
    pub(crate) changed_tasks: Vec<String>,
    pub(crate) header_identity_changed: bool,
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
                        "UPDATE plan_tasks SET position = ?3, updated_at = ?4, content_hash = ?5, document_json = ?6
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
        "INSERT INTO plan_tasks(plan_id, task_id, position, updated_at, content_hash, milestone_count, milestone_digest, document_json)
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
        "INSERT INTO plan_task_milestones(plan_id, task_id, seq, document_json) VALUES (?1, ?2, ?3, ?4)",
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
                    "UPDATE plan_task_attempts SET attempt_id = ?4, content_hash = ?5, document_json = ?6
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
            "INSERT INTO plan_task_attempts(plan_id, task_id, attempt_id, position, content_hash, document_json)
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

/// Joins a plan row with its task rows into the full plan document.
pub(super) fn load_plan_value(
    connection: &Connection,
    plan_id: &str,
    document: &str,
) -> Result<Value, String> {
    let plan: Value = serde_json::from_str(document).map_err(|error| error.to_string())?;
    let mut milestones = grouped_documents(
        connection,
        "SELECT task_id, document_json FROM plan_task_milestones WHERE plan_id = ?1 ORDER BY task_id, seq",
        plan_id,
    )?;
    let mut attempts = grouped_documents(
        connection,
        "SELECT task_id, document_json FROM plan_task_attempts WHERE plan_id = ?1 ORDER BY task_id, position",
        plan_id,
    )?;
    let mut statement = connection
        .prepare_cached(
            "SELECT task_id, document_json FROM plan_tasks WHERE plan_id = ?1 ORDER BY position",
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
    join_plan(plan, tasks)
}

fn grouped_documents(
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

fn parse(document: &str) -> Result<Value, String> {
    serde_json::from_str(document).map_err(|error| error.to_string())
}

pub(super) fn plan_document(
    connection: &Connection,
    plan_id: &str,
) -> rusqlite::Result<Option<String>> {
    connection
        .query_row(
            "SELECT document_json FROM plans WHERE id = ?1",
            [plan_id],
            |row| row.get(0),
        )
        .optional()
}
