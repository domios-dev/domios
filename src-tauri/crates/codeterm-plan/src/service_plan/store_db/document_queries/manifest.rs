use super::{column, db_error, decode, document_metadata_in, record_query, PlanDatabase};
use crate::service_plan::body_files::{
    FileElementInput, FileManifestInput, FileMilestoneInput, FileSectionInput, FileTaskInput,
    MARKDOWN_SIDECAR_MIN_BYTES,
};
use crate::service_plan::{paths, PlanError};
use rusqlite::{params, Connection, OpenFlags};
use std::collections::HashMap;
use std::path::Path;

#[derive(serde::Deserialize)]
struct ElementRow {
    id: String,
    kind: String,
    sequence: String,
    externalize: bool,
}

impl PlanDatabase {
    pub(in crate::service_plan) fn file_manifest_input(
        &self,
        id: &str,
    ) -> Result<FileManifestInput, PlanError> {
        let mut read_connection = Connection::open_with_flags(
            self.path(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(db_error)?;
        read_connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(db_error)?;
        super::super::auxiliary_queries::register_functions(&read_connection)?;
        let connection = read_connection.transaction().map_err(db_error)?;
        let metadata = document_metadata_in(&connection, id)?;
        let directory = paths::plan_state_dir(
            paths::sidecar_root(&metadata.project_path, &metadata.canonical_project_path),
            &metadata.status,
            &metadata.id,
        )
        .map_err(|error| PlanError::Io(error.to_string()))?;
        let milestone_files = existing_names(&directory.join("milestones"))?;
        let element_files = existing_names(&directory.join("elements"))?;
        record_query();
        let has_goal = connection.query_row(
            "SELECT CASE WHEN json_type(header_json,'$.goal') IS NULL OR json_type(header_json,'$.goal')='null' THEN 0
             WHEN json_type(header_json,'$.goal')='object' THEN 1 ELSE json('invalid goal metadata') END FROM plans WHERE id=?1",
            [id],|row| row.get::<_,bool>(0),
        ).map_err(db_error)?;
        record_query();
        let sections = {
            let mut statement = connection.prepare_cached(
                "SELECT json_object('id',json_extract(j.value,'$.id'),'handle',json_extract(j.value,'$.handle'),
                    'title',json_extract(j.value,'$.title')) FROM plan_sections s,json_tree(s.row_json) j
                 WHERE s.plan_id=?1 AND j.type='object' AND (j.fullkey='$' OR j.path LIKE '%.subsections') ORDER BY s.position,j.id"
            ).map_err(db_error)?;
            let rows = statement
                .query_map([id], |row| column::<String>(row, 0))
                .map_err(db_error)?;
            rows.map(|row| decode(id, &row.map_err(db_error)?))
                .collect::<Result<Vec<FileSectionInput>, PlanError>>()?
        };
        record_query();
        let mut tasks = {
            let mut statement = connection.prepare_cached(
                "SELECT json_object('id',json_extract(t.row_json,'$.id'),'handle',json_extract(t.row_json,'$.handle'),
                    'name',json_extract(t.row_json,'$.name'),'has_milestones',json(CASE WHEN EXISTS(
                        SELECT 1 FROM plan_task_milestones m WHERE m.plan_id=t.plan_id AND m.task_id=t.task_id) THEN 'true' ELSE 'false' END))
                 FROM plan_tasks t WHERE t.plan_id=?1 ORDER BY t.position"
            ).map_err(db_error)?;
            let rows = statement
                .query_map([id], |row| column::<String>(row, 0))
                .map_err(db_error)?;
            rows.map(|row| decode(id, &row.map_err(db_error)?))
                .collect::<Result<Vec<FileTaskInput>, PlanError>>()?
        };
        let mut indices = HashMap::new();
        for (index, task) in tasks.iter().enumerate() {
            if indices.insert(task.id.clone(), index).is_some() {
                return Err(PlanError::Quarantined {
                    plan_id: id.into(),
                    reason: "duplicate file manifest task ID".into(),
                });
            }
        }
        record_query();
        {
            let sql = format!(
                "WITH task_facts AS MATERIALIZED (
                    SELECT plan_id,task_id,position,coalesce(json_extract(row_json,'$.handle'),json_extract(row_json,'$.id')) AS handle
                    FROM plan_tasks WHERE plan_id=?1
                 ), source AS (
                    SELECT m.task_id,m.seq,m.row_json AS raw,t.position AS task_position,t.handle,
                        CASE WHEN json_type(m.row_json,'$.text')='text' THEN json_extract(m.row_json,'$.text')
                             ELSE json('invalid milestone markdown') END AS markdown
                    FROM plan_task_milestones m JOIN task_facts t USING(plan_id,task_id) WHERE m.plan_id=?1),
                 eligible AS (SELECT *,handle||'-'||json_extract(raw,'$.id')||'.md' AS filename,{} AS externalize FROM source)
                 SELECT task_id,json_object('id',json_extract(raw,'$.id'),'kind',json_extract(raw,'$.kind'),
                    'externalize',json(CASE WHEN externalize THEN 'true' ELSE 'false' END))
                 FROM eligible WHERE externalize OR filename IN (SELECT value FROM json_each(?4)) ORDER BY task_position,seq",
                externalize_sql("markdown"),
            );
            let mut statement = connection.prepare_cached(&sql).map_err(db_error)?;
            let rows = statement
                .query_map(
                    params![
                        id,
                        super::super::status_queries::WHITESPACE,
                        MARKDOWN_SIDECAR_MIN_BYTES as i64,
                        milestone_files
                    ],
                    |row| Ok((column::<String>(row, 0)?, column::<String>(row, 1)?)),
                )
                .map_err(db_error)?;
            for row in rows {
                let (task_id, document) = row.map_err(db_error)?;
                let index = indices
                    .get(&task_id)
                    .ok_or_else(|| PlanError::Io("manifest milestone has no task".into()))?;
                tasks[*index]
                    .milestones
                    .push(decode::<FileMilestoneInput>(id, &document)?);
            }
        }
        record_query();
        let elements = {
            let sql = format!(
                "WITH source AS (
                    SELECT row_json AS raw,position,json_extract(row_json,'$.kind') AS kind,
                        CASE WHEN json_extract(row_json,'$.kind') IN ('note','decision') AND json_type(row_json,'$.content')='text'
                            THEN json_extract(row_json,'$.content')
                        WHEN json_extract(row_json,'$.kind') IN ('note','decision','discussion') THEN CASE
                            WHEN json_type(row_json,'$.content.text')='text' THEN json_extract(row_json,'$.content.text')
                            WHEN json_type(row_json,'$.content.body')='text' THEN json_extract(row_json,'$.content.body')
                            WHEN json_type(row_json,'$.content.markdown')='text' THEN json_extract(row_json,'$.content.markdown') END END AS markdown
                    FROM plan_elements WHERE plan_id=?1),
                 eligible AS (SELECT *,kind||'-'||json_extract(raw,'$.id')||'.md' AS filename,coalesce({},0) AS externalize FROM source)
                 SELECT json_object('id',json_extract(raw,'$.id'),'kind',kind,
                    'sequence',CASE WHEN kind='handoff' THEN domios_handoff_seq(raw->'$.content.seq') ELSE '0' END,
                    'externalize',json(CASE WHEN externalize THEN 'true' ELSE 'false' END))
                 FROM eligible WHERE kind='handoff' OR externalize OR filename IN (SELECT value FROM json_each(?4)) ORDER BY position",
                externalize_sql("markdown"),
            );
            let mut statement = connection.prepare_cached(&sql).map_err(db_error)?;
            let rows = statement
                .query_map(
                    params![
                        id,
                        super::super::status_queries::WHITESPACE,
                        MARKDOWN_SIDECAR_MIN_BYTES as i64,
                        element_files
                    ],
                    |row| column::<String>(row, 0),
                )
                .map_err(db_error)?;
            rows.map(|row| {
                let element: ElementRow = decode(id, &row.map_err(db_error)?)?;
                Ok(FileElementInput {
                    id: element.id,
                    kind: element.kind,
                    sequence: element.sequence.parse().map_err(
                        |error: std::num::ParseIntError| PlanError::Io(error.to_string()),
                    )?,
                    externalize: element.externalize,
                })
            })
            .collect::<Result<Vec<_>, PlanError>>()?
        };
        connection.commit().map_err(db_error)?;
        Ok(FileManifestInput {
            id: metadata.id,
            directory,
            has_goal,
            sections,
            tasks,
            elements,
        })
    }
}

fn existing_names(directory: &Path) -> Result<String, PlanError> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok("[]".into()),
        Err(error) => return Err(PlanError::Io(error.to_string())),
    };
    let names = entries
        .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| PlanError::Io(error.to_string()))?;
    serde_json::to_string(&names).map_err(super::super::json_error)
}

fn externalize_sql(markdown: &str) -> String {
    format!(
        "(length(CAST(trim({markdown},?2) AS BLOB))>=?3 OR instr(trim({markdown},?2),char(10))>0)"
    )
}
