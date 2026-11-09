use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
use crate::plan::{
    CanvasElement, Milestone, OrchestratorHeartbeat, PlanStatus, Section, Spec, Task,
    TaskExecutionAttempt,
};
use crate::service_plan::export_projection::header::PlanDocumentHeader;
use crate::service_plan::export_projection::{JsonPlanExport, PlanExportEncoder};
use crate::service_plan::PlanError;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::collections::HashMap;

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
        let mut connection = Connection::open_with_flags(
            self.path(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(db_error)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(db_error)?;
        let transaction = connection.transaction().map_err(db_error)?;
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

fn document(
    connection: &Connection,
    id: &str,
    encoder: &impl PlanExportEncoder,
) -> Result<Value, PlanError> {
    record_query();
    let header: Option<Option<String>> = connection.query_row(
        "SELECT CASE WHEN json_type(header_json,'$.tasks') IS NULL
            AND json_type(header_json,'$.sections') IS NULL
            AND json_type(header_json,'$.elements') IS NULL
            AND json_type(header_json,'$.strides') IS NULL
            AND json_type(header_json,'$.heartbeat') IS NULL THEN header_json END FROM plans WHERE id=?1",
        [id], |row| column(row,0),
    ).optional().map_err(db_error)?;
    let header = header
        .ok_or(PlanError::NotFound)?
        .ok_or_else(|| corrupt(id, "plan header contains separately stored rows"))?;
    let header: PlanDocumentHeader = decode(id, &header)?;
    if header.id != id {
        return Err(corrupt(id, "plan header id differs from its row key"));
    }
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
    record_query();
    let heartbeat: Option<String> = connection
        .query_row(
            "SELECT row_json FROM plan_heartbeat WHERE plan_id=?1",
            [id],
            |row| column(row, 0),
        )
        .optional()
        .map_err(db_error)?;
    let heartbeat: OrchestratorHeartbeat = heartbeat
        .map(|encoded| decode(id, &encoded))
        .transpose()?
        .unwrap_or_default();
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
