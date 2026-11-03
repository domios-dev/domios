use super::{db_error, json_error, queue_operation, PlanDatabase};
use crate::plan::{
    IdentityProvenance, Milestone, MilestoneKind, ReviewOverride, Task, TaskExecutionAttempt,
};
use crate::service_plan::{store_rows, text_bounds, PlanError};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use std::collections::BTreeSet;

pub(in crate::service_plan) enum TaskMetadata {
    RoleGuidance(Option<Option<String>>),
    Personality {
        name: Option<Option<String>>,
        provenance: Option<Option<IdentityProvenance>>,
    },
    ReviewOverride(ReviewOverride),
}

pub(in crate::service_plan) enum HotChange {
    Task {
        selector: String,
        change: TaskMetadata,
    },
    Milestone {
        task_id: String,
        milestone: Milestone,
        add_kind: bool,
    },
    Stride(Milestone),
}

#[derive(Debug, Default)]
pub(in crate::service_plan) struct CommittedRows {
    pub plan_id: String,
    pub updated_at: u64,
    pub tasks: BTreeSet<String>,
    pub attempts: usize,
    pub milestones: usize,
    pub strides: usize,
    pub task_rows: usize,
    pub header_rows: usize,
    pub decoded_bytes: usize,
    pub serialized_bytes: usize,
    pub reset_tabs: Vec<String>,
    pub search: Vec<crate::service_plan::search_index::MilestoneIndexDelta>,
}

impl PlanDatabase {
    pub(in crate::service_plan) fn commit_hot_changes(
        &self,
        plan_id: &str,
        changes: Vec<HotChange>,
        now_ms: u64,
    ) -> Result<CommittedRows, PlanError> {
        self.hot_transaction(plan_id, now_ms, |tx, receipt| {
            for change in changes {
                match change {
                    HotChange::Task { selector, change } => {
                        let mut task = read_task(tx, plan_id, &selector, receipt)?;
                        match change {
                            TaskMetadata::RoleGuidance(value) => {
                                if let Some(value) = value {
                                    task.role_guidance = value;
                                }
                            }
                            TaskMetadata::Personality { name, provenance } => {
                                if let Some(name) = name {
                                    task.personality = name;
                                }
                                if let Some(provenance) = provenance {
                                    task.personality_provenance = provenance;
                                }
                            }
                            TaskMetadata::ReviewOverride(value) => {
                                task.review_override = Some(value)
                            }
                        }
                        task.updated_at = now_ms;
                        write_task(tx, plan_id, &task, receipt)?;
                    }
                    HotChange::Milestone {
                        task_id,
                        milestone,
                        add_kind,
                    } => {
                        declare_kind(tx, plan_id, &milestone.kind, add_kind, receipt)?;
                        append_milestone(tx, plan_id, &task_id, &milestone, receipt)?;
                    }
                    HotChange::Stride(milestone) => {
                        append_stride(tx, plan_id, &milestone, receipt)?
                    }
                }
            }
            Ok(())
        })
    }

    pub(in crate::service_plan) fn change_attempt<T>(
        &self,
        plan_id: &str,
        selector: &str,
        tab_id: &str,
        attempt_id: Option<&str>,
        now_ms: u64,
        change: impl FnOnce(Option<&mut TaskExecutionAttempt>) -> Result<Option<T>, PlanError>,
    ) -> Result<(Option<T>, CommittedRows), PlanError> {
        let mut result = None;
        let receipt = self.hot_transaction(plan_id, now_ms, |tx, receipt| {
            let task_id = resolve_task(tx, plan_id, selector)?;
            let row: Option<(i64, String)> = tx
                .query_row(
                    "SELECT position, row_json FROM plan_task_attempts
                 WHERE plan_id=?1 AND task_id=?2 AND executor_tab_id=?3
                 AND (?4 IS NULL OR attempt_id=?4) ORDER BY position DESC LIMIT 1",
                    params![plan_id, task_id, tab_id, attempt_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(db_error)?;
            let mut attempt: Option<TaskExecutionAttempt> = row
                .as_ref()
                .map(|(_, json)| {
                    receipt.decoded_bytes += json.len();
                    serde_json::from_str(json).map_err(json_error)
                })
                .transpose()?;
            result = change(attempt.as_mut())?;
            if result.is_none() {
                return Ok(());
            }
            let Some((position, _)) = row else {
                return Err(PlanError::TaskNotFound);
            };
            let document = encode(attempt.as_ref().ok_or(PlanError::TaskNotFound)?, receipt)?;
            tx.execute(
                "UPDATE plan_task_attempts SET row_json=?4, content_hash=?5
                 WHERE plan_id=?1 AND task_id=?2 AND position=?3",
                params![
                    plan_id,
                    task_id,
                    position,
                    document,
                    store_rows::row_hash(&document)
                ],
            )
            .map_err(db_error)?;
            receipt.attempts += 1;
            let mut task = read_task(tx, plan_id, &task_id, receipt)?;
            task.updated_at = now_ms;
            write_task(tx, plan_id, &task, receipt)
        })?;
        Ok((result, receipt))
    }

    fn hot_transaction(
        &self,
        plan_id: &str,
        now_ms: u64,
        change: impl FnOnce(&Transaction<'_>, &mut CommittedRows) -> Result<(), PlanError>,
    ) -> Result<CommittedRows, PlanError> {
        let mut connection = self.connection.lock();
        let operation_id = queue_operation(&connection, plan_id, "persist_plan", now_ms)?;
        let result = (|| {
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(db_error)?;
            if !tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1)",
                    [plan_id],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(db_error)?
            {
                return Err(PlanError::NotFound);
            }
            tx.execute(
                "UPDATE plan_operations SET state='running' WHERE id=?1",
                [operation_id],
            )
            .map_err(db_error)?;
            let mut receipt = CommittedRows {
                plan_id: plan_id.into(),
                updated_at: now_ms,
                ..Default::default()
            };
            change(&tx, &mut receipt)?;
            if receipt.task_rows + receipt.milestones + receipt.attempts + receipt.strides > 0 {
                tx.execute("UPDATE plans SET updated_at=?2, header_json=json_set(header_json, '$.updated_at', ?2) WHERE id=?1", params![plan_id, now_ms]).map_err(db_error)?;
                receipt.header_rows += 1;
                let mut statement = tx.prepare_cached("SELECT DISTINCT assigned_tab_id FROM plan_tasks WHERE plan_id=?1 AND assigned_tab_id IS NOT NULL").map_err(db_error)?;
                receipt.reset_tabs = statement
                    .query_map([plan_id], |row| row.get(0))
                    .map_err(db_error)?
                    .collect::<Result<_, _>>()
                    .map_err(db_error)?;
            }
            tx.execute(
                "UPDATE plan_operations SET state='committed', finished_at=?2 WHERE id=?1",
                params![operation_id, now_ms],
            )
            .map_err(db_error)?;
            tx.commit().map_err(db_error)?;
            Ok(receipt)
        })();
        if let Err(error) = &result {
            let _ = connection.execute(
                "UPDATE plan_operations SET state='failed', finished_at=?2, error=?3 WHERE id=?1",
                params![operation_id, now_ms, error.to_string()],
            );
        }
        result
    }
}

fn resolve_task(tx: &Transaction<'_>, plan_id: &str, selector: &str) -> Result<String, PlanError> {
    let exact = tx
        .query_row(
            "SELECT task_id FROM plan_tasks WHERE plan_id=?1 AND task_id=?2",
            params![plan_id, selector],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(db_error)?;
    if let Some(id) = exact {
        return Ok(id);
    }
    let mut statement = tx.prepare_cached("SELECT task_id, json_extract(row_json, '$.key'), json_extract(row_json, '$.handle') FROM plan_tasks WHERE plan_id=?1 ORDER BY position").map_err(db_error)?;
    let mut found = None;
    let rows = statement
        .query_map([plan_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(db_error)?;
    for row in rows {
        let (id, key, handle) = row.map_err(db_error)?;
        let key = key.unwrap_or_else(|| crate::plan::task_key::fallback_key(&id));
        if crate::plan::task_key::name_matches(handle.as_deref(), &key, selector) {
            if found.is_some() {
                return Err(PlanError::TaskNotFound);
            }
            found = Some(id);
        }
    }
    found.ok_or(PlanError::TaskNotFound)
}

fn read_task(
    tx: &Transaction<'_>,
    plan_id: &str,
    selector: &str,
    receipt: &mut CommittedRows,
) -> Result<Task, PlanError> {
    let id = resolve_task(tx, plan_id, selector)?;
    let json: String = tx
        .query_row(
            "SELECT row_json FROM plan_tasks WHERE plan_id=?1 AND task_id=?2",
            params![plan_id, id],
            |row| row.get(0),
        )
        .map_err(db_error)?;
    receipt.decoded_bytes += json.len();
    serde_json::from_str(&json).map_err(json_error)
}

fn encode(value: &impl serde::Serialize, receipt: &mut CommittedRows) -> Result<String, PlanError> {
    let value = serde_json::to_value(value).map_err(json_error)?;
    text_bounds::validate_stored_entity(&value)?;
    let document = serde_json::to_string(&value).map_err(json_error)?;
    if document.len() > store_rows::MAX_STORED_ROW_BYTES {
        return Err(PlanError::InvalidPlanText(format!(
            "entity row exceeds {} UTF-8 bytes",
            store_rows::MAX_STORED_ROW_BYTES
        )));
    }
    receipt.serialized_bytes += document.len();
    Ok(document)
}

fn write_task(
    tx: &Transaction<'_>,
    plan_id: &str,
    task: &Task,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    text_bounds::validate_stored_task_text(task)?;
    let mut value = serde_json::to_value(task).map_err(json_error)?;
    if let Some(fields) = value.as_object_mut() {
        fields.remove("milestones");
        fields.remove("execution_attempts");
    }
    let document = encode(&value, receipt)?;
    tx.execute("UPDATE plan_tasks SET updated_at=?3, content_hash=?4, row_json=?5 WHERE plan_id=?1 AND task_id=?2", params![plan_id, task.id, task.updated_at, store_rows::row_hash(&document), document]).map_err(db_error)?;
    receipt.tasks.insert(task.id.clone());
    receipt.task_rows += 1;
    Ok(())
}

fn declare_kind(
    tx: &Transaction<'_>,
    plan_id: &str,
    kind: &str,
    add: bool,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    let json: String = tx.query_row("SELECT coalesce(json_extract(header_json, '$.milestone_kinds'), '[]') FROM plans WHERE id=?1", [plan_id], |row| row.get(0)).map_err(db_error)?;
    receipt.decoded_bytes += json.len();
    let mut kinds: Vec<MilestoneKind> = serde_json::from_str(&json).map_err(json_error)?;
    if kinds.iter().any(|entry| entry.key == kind) {
        return Ok(());
    }
    if !add {
        return Err(PlanError::UndeclaredKind {
            declared: kinds.into_iter().map(|entry| entry.key).collect(),
        });
    }
    kinds.push(MilestoneKind {
        key: kind.into(),
        label: kind.into(),
        color: None,
    });
    let json = encode(&kinds, receipt)?;
    tx.execute("UPDATE plans SET header_json=json_set(header_json, '$.milestone_kinds', json(?2)) WHERE id=?1", params![plan_id, json]).map_err(db_error)?;
    receipt.header_rows += 1;
    Ok(())
}

fn append_milestone(
    tx: &Transaction<'_>,
    plan_id: &str,
    task_id: &str,
    milestone: &Milestone,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    let (count, digest): (i64, i64) = tx.query_row("SELECT milestone_count, milestone_digest FROM plan_tasks WHERE plan_id=?1 AND task_id=?2", params![plan_id, task_id], |row| Ok((row.get(0)?, row.get(1)?))).optional().map_err(db_error)?.ok_or(PlanError::TaskNotFound)?;
    let json = encode(milestone, receipt)?;
    tx.execute(
        "INSERT INTO plan_task_milestones(plan_id, task_id, seq, row_json) VALUES (?1, ?2, ?3, ?4)",
        params![plan_id, task_id, count, json],
    )
    .map_err(db_error)?;
    tx.execute("UPDATE plan_tasks SET milestone_count=?3, milestone_digest=?4 WHERE plan_id=?1 AND task_id=?2", params![plan_id, task_id, count + 1, store_rows::chain_hash(digest, &json)]).map_err(db_error)?;
    receipt.tasks.insert(task_id.into());
    receipt.task_rows += 1;
    receipt.milestones += 1;
    let delta = tx
        .query_row(
            "SELECT p.title, p.status, p.project_path,
         json_extract(p.header_json, '$.canonical_project_path'),
         json_extract(t.row_json, '$.name'), t.status
         FROM plans p JOIN plan_tasks t ON t.plan_id=p.id WHERE p.id=?1 AND t.task_id=?2",
            params![plan_id, task_id],
            |row| {
                Ok(crate::service_plan::search_index::MilestoneIndexDelta {
                    plan_id: plan_id.into(),
                    title: row.get(0)?,
                    plan_status: row.get(1)?,
                    project_path: row.get(2)?,
                    canonical_project_path: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    task_name: row.get(4)?,
                    task_status: row.get(5)?,
                    milestone: milestone.clone(),
                })
            },
        )
        .map_err(db_error)?;
    receipt.decoded_bytes += delta.title.len()
        + delta.plan_status.len()
        + delta.project_path.len()
        + delta.canonical_project_path.len()
        + delta.task_name.len()
        + delta.task_status.len();
    receipt.search.push(delta);
    Ok(())
}

fn append_stride(
    tx: &Transaction<'_>,
    plan_id: &str,
    milestone: &Milestone,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    let (count, digest): (i64, i64) = tx
        .query_row(
            "SELECT stride_count, stride_digest FROM plans WHERE id=?1",
            [plan_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(db_error)?;
    let json = encode(milestone, receipt)?;
    tx.execute(
        "INSERT INTO plan_strides(plan_id, seq, row_json) VALUES (?1, ?2, ?3)",
        params![plan_id, count, json],
    )
    .map_err(db_error)?;
    tx.execute(
        "UPDATE plans SET stride_count=?2, stride_digest=?3 WHERE id=?1",
        params![plan_id, count + 1, store_rows::chain_hash(digest, &json)],
    )
    .map_err(db_error)?;
    receipt.header_rows += 1;
    receipt.strides += 1;
    Ok(())
}

#[cfg(test)]
mod tests;
