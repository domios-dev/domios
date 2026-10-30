use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

pub(super) const MAX_STORED_ROW_BYTES: usize = 4 * 1024 * 1024;
const TASKS: &str = "tasks";
const MILESTONES: &str = "milestones";
const ATTEMPTS: &str = "execution_attempts";

#[derive(Debug)]
pub(super) struct PlanRows {
    pub(super) plan: String,
    pub(super) tasks: Vec<TaskRows>,
}

#[derive(Debug)]
pub(super) struct TaskRows {
    pub(super) id: String,
    pub(super) updated_at: i64,
    pub(super) document: String,
    pub(super) hash: i64,
    pub(super) milestones: Vec<String>,
    /// `milestone_digests[k]` chains the hashes of milestones `0..=k`, so a stored prefix is verifiable.
    pub(super) milestone_digests: Vec<i64>,
    pub(super) attempts: Vec<AttemptRow>,
}

impl TaskRows {
    pub(super) fn milestone_digest(&self) -> i64 {
        self.milestone_digests.last().copied().unwrap_or(0)
    }
}

#[derive(Debug)]
pub(super) struct AttemptRow {
    pub(super) id: String,
    pub(super) document: String,
    pub(super) hash: i64,
}

/// Splits a plan document into its plan row and uniquely keyed task rows.
pub(super) fn split_plan(mut plan: Value) -> Result<PlanRows, String> {
    let fields = plan
        .as_object_mut()
        .ok_or("plan document is not an object")?;
    if let Some(reason) = fields.get(TASKS).and_then(unkeyable_tasks) {
        return Err(reason);
    }
    let tasks = match fields.remove(TASKS) {
        Some(Value::Array(tasks)) => tasks,
        _ => Vec::new(),
    };
    let tasks = tasks
        .into_iter()
        .map(split_task)
        .collect::<Result<Vec<_>, String>>()?;
    Ok(PlanRows {
        plan: to_json(&plan)?,
        tasks,
    })
}

fn unkeyable_tasks(tasks: &Value) -> Option<String> {
    let Some(tasks) = tasks.as_array() else {
        return Some("plan tasks are not an array".into());
    };
    let mut seen = HashSet::new();
    for task in tasks {
        let Some(id) = task.get("id").and_then(Value::as_str) else {
            return Some("a task has no string id".into());
        };
        if !seen.insert(id) {
            return Some(format!("duplicate task id {id}"));
        }
    }
    None
}

fn split_task(mut task: Value) -> Result<TaskRows, String> {
    let fields = task.as_object_mut().ok_or("task is not an object")?;
    let id = fields
        .get("id")
        .and_then(Value::as_str)
        .ok_or("task has no string id")?
        .to_string();
    let updated_at = fields
        .get("updated_at")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let milestones = take_array(fields, MILESTONES, &id)?;
    let attempts = take_array(fields, ATTEMPTS, &id)?;
    let document = to_json(&task)?;
    let mut digest = 0;
    let mut milestone_digests = Vec::with_capacity(milestones.len());
    let milestones = milestones
        .iter()
        .map(|milestone| {
            let document = to_json(milestone)?;
            digest = chain_hash(digest, &document);
            milestone_digests.push(digest);
            Ok(document)
        })
        .collect::<Result<Vec<_>, String>>()?;
    let attempts = attempts
        .iter()
        .map(|attempt| {
            let attempt_id = attempt
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let document = to_json(attempt)?;
            Ok(AttemptRow {
                id: attempt_id,
                hash: row_hash(&document),
                document,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(TaskRows {
        hash: row_hash(&document),
        id,
        updated_at,
        document,
        milestones,
        milestone_digests,
        attempts,
    })
}

fn take_array(
    fields: &mut Map<String, Value>,
    key: &str,
    task_id: &str,
) -> Result<Vec<Value>, String> {
    match fields.remove(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(values)) => Ok(values),
        Some(_) => Err(format!("task {task_id} {key} is not an array")),
    }
}

pub(super) struct StoredTask {
    pub(super) document: Value,
    pub(super) milestones: Vec<Value>,
    pub(super) attempts: Vec<Value>,
}

/// Rebuilds the plan document from its task rows.
pub(super) fn join_plan(mut plan: Value, tasks: Vec<StoredTask>) -> Result<Value, String> {
    let fields = plan
        .as_object_mut()
        .ok_or("plan document is not an object")?;
    if fields.contains_key(TASKS) {
        return Err("unsupported whole-document plan row: tasks must be stored separately".into());
    }
    let tasks = tasks
        .into_iter()
        .map(|task| {
            let mut document = task.document;
            let fields = document
                .as_object_mut()
                .ok_or("task row is not an object")?;
            fields.insert(MILESTONES.into(), Value::Array(task.milestones));
            fields.insert(ATTEMPTS.into(), Value::Array(task.attempts));
            Ok(document)
        })
        .collect::<Result<Vec<_>, String>>()?;
    fields.insert(TASKS.into(), Value::Array(tasks));
    Ok(plan)
}

pub(super) fn validate_row_sizes(rows: &PlanRows) -> Result<(), String> {
    check_row("plan row", &rows.plan)?;
    for task in &rows.tasks {
        check_row("task row", &task.document)?;
        for milestone in &task.milestones {
            check_row("milestone row", milestone)?;
        }
        for attempt in &task.attempts {
            check_row("attempt row", &attempt.document)?;
        }
    }
    Ok(())
}

fn check_row(kind: &str, document: &str) -> Result<(), String> {
    if document.len() > MAX_STORED_ROW_BYTES {
        return Err(format!("{kind} exceeds {MAX_STORED_ROW_BYTES} UTF-8 bytes"));
    }
    Ok(())
}

pub(super) fn row_hash(document: &str) -> i64 {
    chain_hash(0, document)
}

pub(super) fn chain_hash(previous: i64, document: &str) -> i64 {
    let mut hasher = Sha256::new();
    hasher.update(previous.to_le_bytes());
    hasher.update(document.as_bytes());
    let digest = hasher.finalize();
    i64::from_le_bytes(digest[..8].try_into().expect("sha256 digest has 32 bytes"))
}

fn to_json(value: &Value) -> Result<String, String> {
    serde_json::to_string(value).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests;
