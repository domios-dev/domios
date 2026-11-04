use super::gate_rows as rows;
use super::{
    column, decode, record_query, ReminderAttemptRead, ReminderGateRead, ReminderPlanRead,
};
use crate::service_plan::PlanError;
use rusqlite::{params, Connection};

pub(super) fn load(
    connection: &Connection,
    read: &mut ReminderPlanRead,
    task_filter: Option<&str>,
) -> Result<(), PlanError> {
    let positions = read
        .tasks
        .iter()
        .enumerate()
        .map(|(position, task)| (task.id.clone(), position))
        .collect::<std::collections::HashMap<_, _>>();
    record_query();
    let mut statement = connection
        .prepare_cached(rows::TASK_EVIDENCE)
        .map_err(super::super::super::db_error)?;
    let rows = statement
        .query_map(
            params![
                read.id,
                super::super::super::status_queries::WHITESPACE,
                task_filter
            ],
            |row| {
                Ok((
                    column::<String>(row, 0)?,
                    column::<String>(row, 1)?,
                    column::<String>(row, 2)?,
                    column::<String>(row, 3)?,
                ))
            },
        )
        .map_err(super::super::super::db_error)?;
    #[derive(serde::Deserialize)]
    struct Extra {
        status_changed_at: u64,
        handoff_required: bool,
        blocked_fingerprint: Option<String>,
        last_review: Option<super::ReminderReviewRead>,
        lane_path: Option<String>,
        lane_branch: Option<String>,
    }
    for row in rows {
        let (id, task_hash, review_hash, extra) = row.map_err(super::super::super::db_error)?;
        let task = &mut read.tasks[*positions
            .get(&id)
            .ok_or_else(|| PlanError::Validation("reminder task is missing".into()))?];
        let extra: Extra = decode(&read.id, &extra)?;
        task.projection_hashes
            .insert(format!("task:{id}"), task_hash);
        task.projection_hashes
            .insert(format!("review:{id}"), review_hash);
        task.status_changed_at = extra.status_changed_at;
        task.handoff_required = extra.handoff_required;
        task.blocked_fingerprint = extra.blocked_fingerprint;
        task.last_review = extra.last_review;
        task.lane_path = extra.lane_path;
        task.lane_branch = extra.lane_branch;
    }
    drop(statement);
    record_query();
    let mut statement = connection
        .prepare_cached(rows::ATTEMPT_EVIDENCE)
        .map_err(super::super::super::db_error)?;
    let rows = statement
        .query_map(params![read.id, task_filter], |row| {
            Ok((
                column::<String>(row, 0)?,
                column::<String>(row, 1)?,
                column::<String>(row, 2)?,
                column::<Option<String>>(row, 3)?,
            ))
        })
        .map_err(super::super::super::db_error)?;
    for row in rows {
        let (task_id, document, interruption, report) =
            row.map_err(super::super::super::db_error)?;
        let attempt: ReminderAttemptRead = decode(&read.id, &document)?;
        let task = &mut read.tasks[*positions
            .get(&task_id)
            .ok_or_else(|| PlanError::Validation("reminder attempt task is missing".into()))?];
        task.projection_hashes
            .insert(format!("attempt:{}", attempt.id), interruption);
        if let (Some(id), Some(hash)) = (&attempt.report_id, report) {
            task.projection_hashes.insert(format!("report:{id}"), hash);
        }
        task.attempts.push(attempt);
    }
    drop(statement);
    record_query();
    let mut statement = connection
        .prepare_cached(rows::TASK_MILESTONE_HASHES)
        .map_err(super::super::super::db_error)?;
    let rows = statement
        .query_map(params![read.id, task_filter], |row| {
            Ok((
                column::<String>(row, 0)?,
                column::<String>(row, 1)?,
                column::<String>(row, 2)?,
            ))
        })
        .map_err(super::super::super::db_error)?;
    for row in rows {
        let (task, id, hash) = row.map_err(super::super::super::db_error)?;
        read.tasks[*positions
            .get(&task)
            .ok_or_else(|| PlanError::Validation("reminder milestone task is missing".into()))?]
        .projection_hashes
        .insert(format!("stride:{id}"), hash);
    }
    read.projection
        .insert("mode".into(), format!("{:?}:{:?}", read.mode, read.status));
    for task in &read.tasks {
        read.projection.extend(task.projection_hashes.clone());
    }
    drop(statement);
    if task_filter.is_some() {
        return Ok(());
    }
    record_query();
    let mut statement = connection
        .prepare_cached(rows::PLAN_STRIDE_HASHES)
        .map_err(super::super::super::db_error)?;
    let rows = statement
        .query_map([&read.id], |row| {
            Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
        })
        .map_err(super::super::super::db_error)?;
    for row in rows {
        let (id, hash) = row.map_err(super::super::super::db_error)?;
        read.projection.insert(format!("stride:{id}"), hash);
    }
    drop(statement);
    record_query();
    let mut statement = connection
        .prepare_cached(rows::GATE_HASHES)
        .map_err(super::super::super::db_error)?;
    let rows = statement
        .query_map([&read.id], |row| {
            Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
        })
        .map_err(super::super::super::db_error)?;
    for row in rows {
        let (document, hash) = row.map_err(super::super::super::db_error)?;
        let gate: ReminderGateRead = decode(&read.id, &document)?;
        read.projection.insert(format!("gate:{}", gate.id), hash);
        read.gates.push(gate);
    }
    drop(statement);
    record_query();
    let mut statement = connection
        .prepare_cached(rows::LEARNING_CATALOG)
        .map_err(super::super::super::db_error)?;
    let rows = statement
        .query_map(
            params![read.id, super::super::super::status_queries::WHITESPACE],
            |row| Ok((column::<String>(row, 0)?, column::<String>(row, 1)?)),
        )
        .map_err(super::super::super::db_error)?;
    for row in rows {
        let (id, text) = row.map_err(super::super::super::db_error)?;
        read.learning_catalog.insert(id, text);
    }
    Ok(())
}

pub(super) fn context(
    connection: &Connection,
    read: &ReminderPlanRead,
    delta: &std::collections::BTreeMap<String, String>,
    limit: usize,
) -> Result<(std::collections::BTreeMap<String, serde_json::Value>, bool), PlanError> {
    record_query();
    let delta = serde_json::to_string(delta).map_err(|error| PlanError::Io(error.to_string()))?;
    let tasks =
        serde_json::to_string(&read.tasks).map_err(|error| PlanError::Io(error.to_string()))?;
    let mut statement = connection
        .prepare_cached(rows::DELTA_CONTEXT)
        .map_err(super::super::super::db_error)?;
    let rows = statement
        .query_map(
            params![read.id, delta, tasks, limit.saturating_add(1)],
            |row| Ok((column::<String>(row, 0)?, column::<String>(row, 1)?)),
        )
        .map_err(super::super::super::db_error)?;
    let mut context = std::collections::BTreeMap::new();
    let mut truncated = false;
    for row in rows {
        let (key, body) = row.map_err(super::super::super::db_error)?;
        if context.len() == limit {
            truncated = true;
            break;
        }
        context.insert(key, decode(&read.id, &body)?);
    }
    Ok((context, truncated))
}
