use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
use crate::plan::{
    CanvasElement, Milestone, OrchestratorHeartbeat, PlanStatus, Section, Spec, Task,
    TaskExecutionAttempt,
};
use crate::service_plan::export_projection::header::PlanDocumentHeader;
use crate::service_plan::export_projection::{
    BoundedPlanExportLimits, JsonPlanExport, PlanExportEncoder,
};
use crate::service_plan::PlanError;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

impl PlanDatabase {
    pub(in crate::service_plan) fn spec_full(&self, id: &str) -> Result<Option<Value>, PlanError> {
        self.export_snapshot(|connection| {
            let Some(spec) = spec_document(connection, id)? else {
                return Ok(None);
            };
            let coverage = spec_coverage(connection, id)?;
            let links = super::coverage_queries::spec_links(connection, id)?;
            Ok(Some(
                serde_json::json!({"spec":spec,"coverage":coverage,"links":links}),
            ))
        })
    }

    pub(in crate::service_plan) fn effort_bundle(
        &self,
        id: &str,
    ) -> Result<Option<Value>, PlanError> {
        self.export_snapshot(|connection| {
            let plan = match document(connection, id, &JsonPlanExport) {
                Ok(plan) => plan,
                Err(PlanError::NotFound) => return Ok(None),
                Err(error) => return Err(error),
            };
            let references = plan["spec_refs"]
                .as_array()
                .ok_or_else(|| corrupt(id, "exported spec references are not an array"))?;
            let mut specs = Vec::new();
            let mut coverage = Vec::new();
            for reference in references {
                let reference = reference
                    .as_str()
                    .ok_or_else(|| corrupt(id, "exported spec reference is not a string"))?;
                let Some(spec) = spec_document(connection, reference)? else {
                    continue;
                };
                let report = spec_coverage(connection, reference)?;
                coverage.push(serde_json::json!({"spec_id":spec.id,"coverage":report}));
                specs.push(spec);
            }
            Ok(Some(
                serde_json::json!({"plan":plan,"specs":specs,"spec_coverage":coverage}),
            ))
        })
    }

    pub(in crate::service_plan) fn export_plan_document(
        &self,
        id: &str,
    ) -> Result<Value, PlanError> {
        self.export_snapshot(|connection| document(connection, id, &JsonPlanExport))
    }

    pub(in crate::service_plan) fn export_plan_document_bounded(
        &self,
        id: &str,
        limits: BoundedPlanExportLimits,
    ) -> Result<Value, PlanError> {
        self.export_snapshot(|connection| document_bounded(connection, id, &JsonPlanExport, limits))
    }

    pub(in crate::service_plan) fn export_plan_documents(
        &self,
        ids: &[String],
    ) -> Result<Vec<Value>, PlanError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        self.export_snapshot(|connection| {
            ids.iter()
                .map(|id| document(connection, id, &JsonPlanExport))
                .collect()
        })
    }

    pub(in crate::service_plan) fn export_all_plan_documents(
        &self,
    ) -> Result<Vec<Value>, PlanError> {
        self.export_all_plan_documents_with(&JsonPlanExport)
    }

    pub(in crate::service_plan) fn export_all_plan_documents_with(
        &self,
        encoder: &impl PlanExportEncoder,
    ) -> Result<Vec<Value>, PlanError> {
        self.export_snapshot(|connection| {
            record_query();
            let mut statement = connection
                .prepare_cached("SELECT id FROM plans ORDER BY id")
                .map_err(db_error)?;
            let ids = statement
                .query_map([], |row| column::<String>(row, 0))
                .map_err(db_error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(db_error)?;
            ids.iter()
                .map(|id| document(connection, id, encoder))
                .collect()
        })
    }

    pub(in crate::service_plan) fn export_plan_list(
        &self,
        project: Option<&str>,
        status: Option<PlanStatus>,
        unowned: bool,
        owner: Option<&str>,
    ) -> Result<Vec<Value>, PlanError> {
        let status = super::index_queries::status_filter(status.as_ref());
        self.export_snapshot(|connection| {
            let ids = super::index_queries::filtered_plan_ids(
                connection, project, status, unowned, owner,
            )?;
            ids.iter()
                .map(|id| document(connection, id, &JsonPlanExport))
                .collect()
        })
    }

    fn export_snapshot<T>(
        &self,
        read: impl FnOnce(&Connection) -> Result<T, PlanError>,
    ) -> Result<T, PlanError> {
        let mut guard = self.read_connection();
        let transaction = guard.transaction().map_err(db_error)?;
        let result = read(&transaction)?;
        transaction.commit().map_err(db_error)?;
        Ok(result)
    }
}

fn spec_document(connection: &Connection, id: &str) -> Result<Option<Spec>, PlanError> {
    record_query();
    let row: Option<String> = connection
        .query_row("SELECT row_json FROM specs WHERE id=?1", [id], |row| {
            column(row, 0)
        })
        .optional()
        .map_err(db_error)?;
    let spec: Option<Spec> = row.map(|row| decode(id, &row)).transpose()?;
    if spec.as_ref().is_some_and(|spec| spec.id != id) {
        return Err(corrupt(id, "spec id differs from its row key"));
    }
    Ok(spec)
}

fn spec_coverage(
    connection: &Connection,
    id: &str,
) -> Result<Option<crate::plan::coverage::SpecCoverageReport>, PlanError> {
    super::coverage_queries::spec_coverage_input(connection, id).map(|input| {
        input.map(|(spec, plans)| crate::plan::coverage::compute_spec_coverage_report(spec, &plans))
    })
}

fn read_header(connection: &Connection, id: &str) -> Result<PlanDocumentHeader, PlanError> {
    record_query();
    let header: Option<Option<String>> = connection
        .query_row(
            "SELECT CASE WHEN json_type(header_json,'$.tasks') IS NULL
            AND json_type(header_json,'$.sections') IS NULL
            AND json_type(header_json,'$.elements') IS NULL
            AND json_type(header_json,'$.strides') IS NULL
            AND json_type(header_json,'$.heartbeat') IS NULL THEN header_json END FROM plans WHERE id=?1",
            [id],
            |row| column(row, 0),
        )
        .optional()
        .map_err(db_error)?;
    let header = header
        .ok_or(PlanError::NotFound)?
        .ok_or_else(|| corrupt(id, "plan header contains separately stored rows"))?;
    let header: PlanDocumentHeader = decode(id, &header)?;
    if header.id != id {
        return Err(corrupt(id, "plan header id differs from its row key"));
    }
    Ok(header)
}

fn read_heartbeat(connection: &Connection, id: &str) -> Result<OrchestratorHeartbeat, PlanError> {
    record_query();
    let heartbeat: Option<String> = connection
        .query_row(
            "SELECT row_json FROM plan_heartbeat WHERE plan_id=?1",
            [id],
            |row| column(row, 0),
        )
        .optional()
        .map_err(db_error)?;
    Ok(heartbeat
        .map(|encoded| decode(id, &encoded))
        .transpose()?
        .unwrap_or_default())
}

fn document(
    connection: &Connection,
    id: &str,
    encoder: &impl PlanExportEncoder,
) -> Result<Value, PlanError> {
    let header = read_header(connection, id)?;
    let (mut tasks, positions) = task_rows(connection, id, encoder)?;
    append_task_rows::<Milestone>(
        connection,
        id,
        "SELECT task_id,row_json FROM plan_task_milestones WHERE plan_id=?1 ORDER BY task_id,seq",
        "milestones",
        &mut tasks,
        &positions,
        encoder,
    )?;
    append_task_rows::<TaskExecutionAttempt>(connection,id,
        "SELECT task_id,row_json FROM plan_task_attempts WHERE plan_id=?1 ORDER BY task_id,position",
        "execution_attempts",&mut tasks,&positions,encoder)?;
    let sections = ordered_rows::<Section>(
        connection,
        id,
        "SELECT row_json FROM plan_sections WHERE plan_id=?1 ORDER BY position",
        encoder,
    )?;
    let elements = ordered_rows::<CanvasElement>(
        connection,
        id,
        "SELECT row_json FROM plan_elements WHERE plan_id=?1 ORDER BY position",
        encoder,
    )?;
    let strides = ordered_rows::<Milestone>(
        connection,
        id,
        "SELECT row_json FROM plan_strides WHERE plan_id=?1 ORDER BY seq",
        encoder,
    )?;
    let heartbeat = read_heartbeat(connection, id)?;
    let mut result = json_value(header, encoder)?;
    let fields = result
        .as_object_mut()
        .ok_or_else(|| corrupt(id, "export header is not an object"))?;
    fields.insert("tasks".into(), Value::Array(tasks));
    fields.insert("sections".into(), Value::Array(sections));
    fields.insert("elements".into(), Value::Array(elements));
    fields.insert("strides".into(), Value::Array(strides));
    fields.insert("heartbeat".into(), json_value(heartbeat, encoder)?);
    Ok(result)
}

fn document_bounded(
    connection: &Connection,
    id: &str,
    encoder: &impl PlanExportEncoder,
    limits: BoundedPlanExportLimits,
) -> Result<Value, PlanError> {
    let header = read_header(connection, id)?;
    record_query();
    let total_tasks: usize = connection
        .query_row(
            "SELECT COUNT(*) FROM plan_tasks INDEXED BY plan_tasks_status_summary WHERE plan_id=?1",
            [id],
            |row| row.get(0),
        )
        .map_err(db_error)?;
    record_query();
    let total_strides: usize = connection
        .query_row(
            "SELECT COUNT(*) FROM plan_strides WHERE plan_id=?1",
            [id],
            |row| row.get(0),
        )
        .map_err(db_error)?;
    let selected_ids = select_bounded_task_ids(connection, id, limits.max_tasks)?;
    let (mut tasks, ordered_ids) = selected_task_rows(connection, id, &selected_ids, encoder)?;
    let mut task_history_truncated = false;
    for (index, task_id) in ordered_ids.iter().enumerate() {
        let milestones_truncated = append_bounded_task_rows::<Milestone>(
            connection,
            id,
            task_id,
            "SELECT row_json FROM plan_task_milestones WHERE plan_id=?1 AND task_id=?2 ORDER BY seq DESC LIMIT ?3",
            "milestones",
            &mut tasks[index],
            limits.max_task_milestones,
            encoder,
        )?;
        let attempts_truncated = append_bounded_task_rows::<TaskExecutionAttempt>(
            connection,
            id,
            task_id,
            "SELECT row_json FROM plan_task_attempts WHERE plan_id=?1 AND task_id=?2 ORDER BY position DESC LIMIT ?3",
            "execution_attempts",
            &mut tasks[index],
            limits.max_task_attempts,
            encoder,
        )?;
        task_history_truncated |= milestones_truncated || attempts_truncated;
    }
    let sections = ordered_rows::<Section>(
        connection,
        id,
        "SELECT row_json FROM plan_sections WHERE plan_id=?1 ORDER BY position",
        encoder,
    )?;
    let elements = ordered_rows::<CanvasElement>(
        connection,
        id,
        "SELECT row_json FROM plan_elements WHERE plan_id=?1 ORDER BY position",
        encoder,
    )?;
    let strides = tail_ordered_rows::<Milestone>(
        connection,
        id,
        "SELECT row_json FROM plan_strides WHERE plan_id=?1 ORDER BY seq DESC LIMIT ?2",
        limits.max_strides,
        encoder,
    )?;
    let heartbeat = read_heartbeat(connection, id)?;
    let tasks_returned = tasks.len();
    let strides_returned = strides.len();
    let truncated =
        total_tasks > tasks_returned || total_strides > strides_returned || task_history_truncated;
    let mut result = json_value(header, encoder)?;
    let fields = result
        .as_object_mut()
        .ok_or_else(|| corrupt(id, "export header is not an object"))?;
    fields.insert("tasks".into(), Value::Array(tasks));
    fields.insert("sections".into(), Value::Array(sections));
    fields.insert("elements".into(), Value::Array(elements));
    fields.insert("strides".into(), Value::Array(strides));
    fields.insert("heartbeat".into(), json_value(heartbeat, encoder)?);
    if truncated {
        fields.insert(
            "_bounded".into(),
            serde_json::json!({
                "truncated": true,
                "tasks_total": total_tasks,
                "tasks_returned": tasks_returned,
                "strides_total": total_strides,
                "strides_returned": strides_returned,
                "task_milestones_limit": limits.max_task_milestones,
                "task_attempts_limit": limits.max_task_attempts,
                "task_history_truncated": task_history_truncated,
            }),
        );
    }
    Ok(result)
}

fn select_bounded_task_ids(
    connection: &Connection,
    id: &str,
    limit: usize,
) -> Result<Vec<String>, PlanError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    record_query();
    let mut open_stmt = connection
        .prepare_cached(
            "SELECT task_id FROM plan_tasks INDEXED BY plan_tasks_status_summary
             WHERE plan_id=?1
               AND json_extract(row_json,'$.status') IN ('in_progress','in_review','blocked','pending','paused')
             LIMIT ?2",
        )
        .map_err(db_error)?;
    let mut selected: Vec<String> = open_stmt
        .query_map(params![id, limit as i64], |row| column::<String>(row, 0))
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)?;
    if selected.len() >= limit {
        return Ok(selected);
    }
    let mut seen: HashSet<String> = selected.iter().cloned().collect();
    let fetch = limit.saturating_add(selected.len()) as i64;
    record_query();
    let mut recent_stmt = connection
        .prepare_cached(
            "SELECT task_id FROM plan_tasks INDEXED BY plan_tasks_event_order
             WHERE plan_id=?1
             ORDER BY coalesce(json_extract(row_json,'$.created_at'),0) DESC, position DESC
             LIMIT ?2",
        )
        .map_err(db_error)?;
    let recent_rows = recent_stmt
        .query_map(params![id, fetch], |row| column::<String>(row, 0))
        .map_err(db_error)?;
    for row in recent_rows {
        let task_id = row.map_err(db_error)?;
        if seen.insert(task_id.clone()) {
            selected.push(task_id);
            if selected.len() >= limit {
                break;
            }
        }
    }
    Ok(selected)
}

fn selected_task_rows(
    connection: &Connection,
    id: &str,
    selected_ids: &[String],
    encoder: &impl PlanExportEncoder,
) -> Result<(Vec<Value>, Vec<String>), PlanError> {
    if selected_ids.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let encoded_ids = serde_json::to_string(selected_ids).map_err(super::json_error)?;
    record_query();
    let mut statement = connection
        .prepare_cached(
            "SELECT task_id, json_remove(row_json,'$.milestones','$.execution_attempts')
             FROM plan_tasks
             WHERE plan_id=?1 AND task_id IN (SELECT value FROM json_each(?2))
             ORDER BY position ASC",
        )
        .map_err(db_error)?;
    let rows = statement
        .query_map(params![id, encoded_ids], |row| {
            Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
        })
        .map_err(db_error)?;
    let mut tasks = Vec::with_capacity(selected_ids.len());
    let mut ordered_ids = Vec::with_capacity(selected_ids.len());
    for row in rows {
        let (key, encoded) = row.map_err(db_error)?;
        let task: Task = decode(id, &encoded)?;
        if task.id != key {
            return Err(corrupt(id, "task id differs from its row key"));
        }
        ordered_ids.push(key);
        tasks.push(json_value(task, encoder)?);
    }
    Ok((tasks, ordered_ids))
}

#[allow(clippy::too_many_arguments)]
fn append_bounded_task_rows<T: DeserializeOwned + Serialize>(
    connection: &Connection,
    id: &str,
    task_id: &str,
    sql: &str,
    field: &str,
    task: &mut Value,
    limit: usize,
    encoder: &impl PlanExportEncoder,
) -> Result<bool, PlanError> {
    record_query();
    let mut statement = connection.prepare_cached(sql).map_err(db_error)?;
    let fetch_limit = limit.saturating_add(1) as i64;
    let rows = statement
        .query_map(params![id, task_id, fetch_limit], |row| {
            column::<String>(row, 0)
        })
        .map_err(db_error)?;
    let mut encoded_rows: Vec<String> = rows.collect::<Result<_, _>>().map_err(db_error)?;
    let truncated = encoded_rows.len() > limit;
    if truncated {
        encoded_rows.truncate(limit);
    }
    encoded_rows.reverse();
    if encoded_rows.is_empty() {
        return Ok(truncated);
    }
    let fields = task
        .as_object_mut()
        .ok_or_else(|| corrupt(id, "task export is not an object"))?;
    let records = fields
        .entry(field.to_string())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| corrupt(id, "task history export is not an array"))?;
    for encoded in encoded_rows {
        let value: T = decode(id, &encoded)?;
        records.push(json_value(value, encoder)?);
    }
    Ok(truncated)
}

fn tail_ordered_rows<T: DeserializeOwned + Serialize>(
    connection: &Connection,
    id: &str,
    sql: &str,
    limit: usize,
    encoder: &impl PlanExportEncoder,
) -> Result<Vec<Value>, PlanError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    record_query();
    let mut statement = connection.prepare_cached(sql).map_err(db_error)?;
    let rows = statement
        .query_map(params![id, limit as i64], |row| column::<String>(row, 0))
        .map_err(db_error)?;
    let mut values = rows
        .map(|row| {
            let value: T = decode(id, &row.map_err(db_error)?)?;
            json_value(value, encoder)
        })
        .collect::<Result<Vec<_>, _>>()?;
    values.reverse();
    Ok(values)
}

fn task_rows(
    connection: &Connection,
    id: &str,
    encoder: &impl PlanExportEncoder,
) -> Result<(Vec<Value>, HashMap<String, usize>), PlanError> {
    record_query();
    let mut statement = connection.prepare_cached(
        "SELECT task_id,json_remove(row_json,'$.milestones','$.execution_attempts') FROM plan_tasks WHERE plan_id=?1 ORDER BY position"
    ).map_err(db_error)?;
    let rows = statement
        .query_map([id], |row| {
            Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
        })
        .map_err(db_error)?;
    let mut tasks = Vec::new();
    let mut positions = HashMap::new();
    for row in rows {
        let (key, encoded) = row.map_err(db_error)?;
        let task: Task = decode(id, &encoded)?;
        if task.id != key {
            return Err(corrupt(id, "task id differs from its row key"));
        }
        positions.insert(key, tasks.len());
        tasks.push(json_value(task, encoder)?);
    }
    Ok((tasks, positions))
}

fn append_task_rows<T: DeserializeOwned + Serialize>(
    connection: &Connection,
    id: &str,
    sql: &str,
    field: &str,
    tasks: &mut [Value],
    positions: &HashMap<String, usize>,
    encoder: &impl PlanExportEncoder,
) -> Result<(), PlanError> {
    record_query();
    let mut statement = connection.prepare_cached(sql).map_err(db_error)?;
    let rows = statement
        .query_map([id], |row| {
            Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
        })
        .map_err(db_error)?;
    for row in rows {
        let (task, encoded) = row.map_err(db_error)?;
        let position = positions
            .get(&task)
            .ok_or_else(|| corrupt(id, "history row has no task"))?;
        let value: T = decode(id, &encoded)?;
        let fields = tasks[*position]
            .as_object_mut()
            .ok_or_else(|| corrupt(id, "task export is not an object"))?;
        let records = fields
            .entry(field.to_string())
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| corrupt(id, "task history export is not an array"))?;
        records.push(json_value(value, encoder)?);
    }
    Ok(())
}

fn ordered_rows<T: DeserializeOwned + Serialize>(
    connection: &Connection,
    id: &str,
    sql: &str,
    encoder: &impl PlanExportEncoder,
) -> Result<Vec<Value>, PlanError> {
    record_query();
    let mut statement = connection.prepare_cached(sql).map_err(db_error)?;
    let rows = statement
        .query_map([id], |row| column::<String>(row, 0))
        .map_err(db_error)?;
    rows.map(|row| {
        let value: T = decode(id, &row.map_err(db_error)?)?;
        json_value(value, encoder)
    })
    .collect()
}

fn json_value(value: impl Serialize, encoder: &impl PlanExportEncoder) -> Result<Value, PlanError> {
    encoder.encode(&value)
}

fn corrupt(id: &str, reason: &str) -> PlanError {
    PlanError::Quarantined {
        plan_id: id.into(),
        reason: reason.into(),
    }
}
