use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
use crate::plan::gates::DecisionGate;
use crate::plan::{CanvasElement, PlanStatus};
use crate::service_plan::element_queries::{element_title_fields, ElementSummary};
use crate::service_plan::PlanError;
use rusqlite::{params, Connection, OptionalExtension};

#[derive(serde::Deserialize)]
pub(in crate::service_plan) struct StorageMetadata {
    id: String,
    status: PlanStatus,
    project_path: String,
    canonical_project_path: String,
}

impl StorageMetadata {
    pub(in crate::service_plan) fn state_dir_unchecked(&self) -> std::path::PathBuf {
        crate::service_plan::paths::plan_state_dir_unchecked(
            crate::service_plan::paths::sidecar_root(
                &self.project_path,
                &self.canonical_project_path,
            ),
            &self.status,
            &self.id,
        )
    }
    pub(in crate::service_plan) fn state_dir(&self) -> std::io::Result<std::path::PathBuf> {
        crate::service_plan::paths::plan_state_dir(
            crate::service_plan::paths::sidecar_root(
                &self.project_path,
                &self.canonical_project_path,
            ),
            &self.status,
            &self.id,
        )
    }
}

impl PlanDatabase {
    pub(in crate::service_plan) fn storage_metadata(
        &self,
        id: &str,
    ) -> Result<StorageMetadata, PlanError> {
        storage_metadata(&self.connection.lock(), id)
    }

    pub(in crate::service_plan) fn gate_rows(
        &self,
        id: &str,
        open_only: bool,
        gate_id: Option<&str>,
    ) -> Result<Vec<DecisionGate>, PlanError> {
        read_snapshot(self, id, |connection| {
            record_query();
            let mut statement = connection
                .prepare_cached(
                    "SELECT g.value FROM plans p,json_each(p.header_json,'$.gates') g WHERE p.id=?1
                 AND (?2=0 OR json_extract(g.value,'$.status') IN ('open','escalated'))
                 AND (?3 IS NULL OR json_extract(g.value,'$.id')=?3) ORDER BY g.key LIMIT ?4",
                )
                .map_err(db_error)?;
            let documents = statement
                .query_map(
                    params![
                        id,
                        open_only,
                        gate_id,
                        if gate_id.is_some() { 1 } else { i64::MAX }
                    ],
                    |row| column::<String>(row, 0),
                )
                .map_err(db_error)?;
            documents
                .map(|document| decode(id, &document.map_err(db_error)?))
                .collect()
        })
    }

    pub(in crate::service_plan) fn element_summaries(
        &self,
        id: &str,
        kind: Option<&str>,
    ) -> Result<Vec<ElementSummary>, PlanError> {
        read_snapshot(self, id, |connection| {
            record_query();
            let mut statement=connection.prepare_cached(
                "SELECT json_object('id',json_extract(row_json,'$.id'),'kind',json_extract(row_json,'$.kind'),
                    'links',json(CASE WHEN json_type(row_json,'$.links') IS NULL THEN '[]' ELSE json_extract(row_json,'$.links') END),
                    'canvas',json(CASE WHEN json_type(row_json,'$.canvas') IS NULL THEN '{}' ELSE json_extract(row_json,'$.canvas') END)),
                    domios_element_title(json_extract(row_json,'$.id'),
                        CASE WHEN json_type(row_json,'$.content.title')='text' THEN json_extract(row_json,'$.content.title') END,
                        CASE WHEN json_type(row_json,'$.content.name')='text' THEN json_extract(row_json,'$.content.name') END,
                        CASE WHEN json_type(row_json,'$.content.label')='text' THEN json_extract(row_json,'$.content.label') END,
                        CASE WHEN json_type(row_json,'$.content.text')='text' THEN json_extract(row_json,'$.content.text') END,
                        CASE WHEN json_type(row_json,'$.content.body')='text' THEN json_extract(row_json,'$.content.body') END,
                        CASE WHEN json_type(row_json,'$.content.markdown')='text' THEN json_extract(row_json,'$.content.markdown') END)
                 FROM plan_elements WHERE plan_id=?1 AND (?2 IS NULL OR json_extract(row_json,'$.kind')=?2) ORDER BY position"
            ).map_err(db_error)?;
            let documents = statement
                .query_map(params![id, kind], |row| {
                    Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
                })
                .map_err(db_error)?;
            documents
                .map(|document| {
                    let (document, title) = document.map_err(db_error)?;
                    let element: CanvasElement = decode(id, &document)?;
                    Ok(ElementSummary {
                        id: element.id,
                        kind: element.kind,
                        title,
                        links: element.links,
                        canvas: element.canvas,
                    })
                })
                .collect()
        })
    }

    pub(in crate::service_plan) fn selected_element(
        &self,
        id: &str,
        element_id: &str,
    ) -> Result<CanvasElement, PlanError> {
        read_snapshot(self, id, |connection| {
            record_query();
            let document: String = connection
                .query_row(
                    "SELECT row_json FROM plan_elements WHERE plan_id=?1 AND element_id=?2",
                    params![id, element_id],
                    |row| column(row, 0),
                )
                .optional()
                .map_err(db_error)?
                .ok_or(PlanError::ElementNotFound)?;
            decode(id, &document)
        })
    }

    pub(in crate::service_plan) fn handoff_metadata(
        &self,
        id: &str,
    ) -> Result<Vec<CanvasElement>, PlanError> {
        read_snapshot(self, id, |connection| {
            record_query();
            let mut statement=connection.prepare_cached(
                "SELECT json_object('id',json_extract(row_json,'$.id'),'kind','handoff','content',json_object(
                    'seq',json(CASE WHEN json_type(row_json,'$.content.seq')='integer' THEN row_json->'$.content.seq' ELSE '0' END),
                    'mode',CASE WHEN json_type(row_json,'$.content.mode')='text' THEN json_extract(row_json,'$.content.mode') END,
                    'goal',CASE WHEN json_type(row_json,'$.content.goal')='text' THEN json_extract(row_json,'$.content.goal') END,
                    'ts',json(CASE WHEN json_type(row_json,'$.content.ts')='integer' THEN row_json->'$.content.ts' END),
                    'reseed_state',CASE WHEN json_type(row_json,'$.content.reseed_state')='text' THEN json_extract(row_json,'$.content.reseed_state') END))
                 FROM plan_elements WHERE plan_id=?1 AND json_extract(row_json,'$.kind')='handoff' ORDER BY position"
            ).map_err(db_error)?;
            let documents = statement
                .query_map([id], |row| column::<String>(row, 0))
                .map_err(db_error)?;
            documents
                .map(|document| decode(id, &document.map_err(db_error)?))
                .collect()
        })
    }

    pub(in crate::service_plan) fn selected_handoff(
        &self,
        id: &str,
        selector: Option<&str>,
    ) -> Result<(StorageMetadata, CanvasElement), PlanError> {
        read_snapshot(self, id, |connection| {
            let metadata = storage_metadata(connection, id)?;
            let sequence = selector
                .and_then(|value| value.parse::<u64>().ok())
                .map(|value| format!("{value:020}"));
            record_query();
            let document: String=connection.query_row(
                "SELECT row_json FROM plan_elements WHERE plan_id=?1 AND json_extract(row_json,'$.kind')='handoff'
                 AND (?2 IS NULL OR element_id=?2 OR domios_handoff_seq(row_json->'$.content.seq')=?3)
                 ORDER BY CASE WHEN ?2 IS NULL THEN domios_handoff_seq(row_json->'$.content.seq') END DESC,
                          CASE WHEN ?2 IS NULL THEN position END DESC,position LIMIT 1",
                params![id,selector,sequence],|row|column(row,0),
            ).optional().map_err(db_error)?.ok_or(PlanError::NotFound)?;
            Ok((metadata, decode(id, &document)?))
        })
    }
}

fn read_snapshot<T>(
    database: &PlanDatabase,
    id: &str,
    read: impl FnOnce(&Connection) -> Result<T, PlanError>,
) -> Result<T, PlanError> {
    let mut connection = database.connection.lock();
    let transaction = connection.transaction().map_err(db_error)?;
    record_query();
    let exists: bool = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1)",
            [id],
            |row| row.get(0),
        )
        .map_err(db_error)?;
    if !exists {
        return Err(PlanError::NotFound);
    }
    let value = read(&transaction)?;
    transaction.commit().map_err(db_error)?;
    Ok(value)
}

fn storage_metadata(connection: &Connection, id: &str) -> Result<StorageMetadata, PlanError> {
    record_query();
    let document: String=connection.query_row(
        "SELECT json_object('id',id,'status',status,'project_path',project_path,
            'canonical_project_path',CASE WHEN json_type(header_json,'$.canonical_project_path') IS NULL THEN '' ELSE json_extract(header_json,'$.canonical_project_path') END)
         FROM plans WHERE id=?1",[id],|row|column(row,0),
    ).optional().map_err(db_error)?.ok_or(PlanError::NotFound)?;
    decode(id, &document)
}

pub(super) fn register_functions(connection: &Connection) -> Result<(), PlanError> {
    use rusqlite::functions::FunctionFlags;
    let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;
    connection
        .create_scalar_function("domios_element_title", 7, flags, |context| {
            let id = context
                .get_raw(0)
                .as_str()
                .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?;
            let mut fields = [None; 6];
            for (index, field) in fields.iter_mut().enumerate() {
                if let rusqlite::types::ValueRef::Text(bytes) = context.get_raw(index + 1) {
                    *field =
                        Some(std::str::from_utf8(bytes).map_err(|error| {
                            rusqlite::Error::UserFunctionError(Box::new(error))
                        })?);
                }
            }
            Ok(element_title_fields(id, fields))
        })
        .map_err(db_error)?;
    connection
        .create_scalar_function("domios_handoff_seq", 1, flags, |context| {
            let value = match context.get_raw(0) {
                rusqlite::types::ValueRef::Text(bytes) if bytes.len() <= 20 => {
                    serde_json::from_slice::<u64>(bytes).unwrap_or(0)
                }
                _ => 0,
            };
            Ok(format!("{value:020}"))
        })
        .map_err(db_error)
}
