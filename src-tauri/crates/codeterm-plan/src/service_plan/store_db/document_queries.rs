use super::read_projections::{column, decode, record_query, SECTION_ROWS};
use super::{db_error, PlanDatabase};
use crate::plan::{Goal, LoopSpec};
use crate::service_plan::document_projection::{
    PlanDocumentMetadata, PlannerGateRead, SectionMarkdownRead,
};
use crate::service_plan::hints::PlannerNudgeInput;
use crate::service_plan::PlanError;
use rusqlite::{params, OptionalExtension};

impl PlanDatabase {
    pub(in crate::service_plan) fn planner_gate_context(
        &self,
        id: &str,
        request_id: Option<&str>,
    ) -> Result<Option<PlannerGateRead>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let document: Option<String> = connection
            .query_row(
                "SELECT json_object('owner',json_extract(p.header_json,'$.owner'),
                'planner',json_extract(p.header_json,'$.planner'),
                'request',json((SELECT r.value FROM json_each(p.header_json,'$.planner_requests') r
                    WHERE json_extract(r.value,'$.request_id')=?2 ORDER BY r.key LIMIT 1)),
                'delivery',json((SELECT d.value FROM json_each(p.header_json,'$.planner_inbox') d
                    WHERE json_extract(d.value,'$.request_id')=?2 ORDER BY d.key LIMIT 1)))
             FROM plans p WHERE p.id=?1",
                params![id, request_id],
                |row| column(row, 0),
            )
            .optional()
            .map_err(db_error)?;
        document.map(|document| decode(id, &document)).transpose()
    }

    pub(in crate::service_plan) fn goal_document(
        &self,
        id: &str,
    ) -> Result<Option<Goal>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let document: String = connection
            .query_row(
                "SELECT json_quote(json_extract(header_json,'$.goal')) FROM plans WHERE id=?1",
                [id],
                |row| column(row, 0),
            )
            .optional()
            .map_err(db_error)?
            .ok_or(PlanError::NotFound)?;
        decode(id, &document)
    }

    pub(in crate::service_plan) fn planner_nudge_input(
        &self,
        id: &str,
        caller_tab_id: Option<&str>,
    ) -> Result<Option<PlannerNudgeInput>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let document: Option<String> = connection.query_row(
            "SELECT json_object('id',p.id,'tasks',json((SELECT json_group_array(json(document)) FROM (
                SELECT json_object('name',json_extract(t.row_json,'$.name'),
                    'handle',json_extract(t.row_json,'$.handle'),'status',t.status,
                    'assigned_tab_id',json_extract(t.row_json,'$.assigned_tab_id')) AS document
                FROM plan_tasks t WHERE t.plan_id=p.id AND json_extract(t.row_json,'$.type')='planning'
                    AND t.status NOT IN ('done','cancelled') AND t.assigned_tab_id IS NOT NULL
                    AND (?2 IS NULL OR t.assigned_tab_id<>?2) ORDER BY t.position LIMIT 1))))
             FROM plans p WHERE p.id=?1",
            params![id,caller_tab_id],|row| column(row,0),
        ).optional().map_err(db_error)?;
        document.map(|document| decode(id, &document)).transpose()
    }

    pub(in crate::service_plan) fn document_metadata(
        &self,
        id: &str,
    ) -> Result<PlanDocumentMetadata, PlanError> {
        document_metadata_in(&self.connection.lock(), id)
    }

    pub(in crate::service_plan) fn section_markdown_rows(
        &self,
        id: &str,
    ) -> Result<Vec<SectionMarkdownRead>, PlanError> {
        let mut connection = self.connection.lock();
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
        record_query();
        let sections = {
            let mut statement = transaction.prepare_cached(&format!(
                "SELECT json_object('title',title,'body',coalesce(json_extract(document,'$.body'),''),
                    'depth',1+(length(path)-length(replace(path,'.subsections[','')))/13)
                 FROM ({SECTION_ROWS}) ORDER BY position,traversal"
            )).map_err(db_error)?;
            let rows = statement
                .query_map([id], |row| column::<String>(row, 0))
                .map_err(db_error)?;
            rows.map(|row| decode(id, &row.map_err(db_error)?))
                .collect::<Result<Vec<_>, PlanError>>()?
        };
        transaction.commit().map_err(db_error)?;
        Ok(sections)
    }

    pub(in crate::service_plan) fn loop_plan_id(
        &self,
        loop_id: &str,
    ) -> Result<Option<String>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        connection.query_row(
            "SELECT p.id FROM plans p WHERE EXISTS(SELECT 1 FROM json_each(p.header_json,'$.loops') l
             WHERE json_extract(l.value,'$.id')=?1) ORDER BY p.updated_at DESC,p.id LIMIT 1",
            [loop_id], |row| column(row,0),
        ).optional().map_err(db_error)
    }

    pub(in crate::service_plan) fn loop_status(
        &self,
        loop_id: &str,
    ) -> Result<LoopSpec, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let (id, document): (String, String) = connection
            .query_row(
                "SELECT p.id,l.value FROM plans p,json_each(p.header_json,'$.loops') l
             WHERE json_extract(l.value,'$.id')=?1 ORDER BY p.updated_at DESC,p.id,l.key LIMIT 1",
                [loop_id],
                |row| Ok((column(row, 0)?, column(row, 1)?)),
            )
            .optional()
            .map_err(db_error)?
            .ok_or(PlanError::NotFound)?;
        decode(&id, &document)
    }
}

fn document_metadata_in(
    connection: &rusqlite::Connection,
    id: &str,
) -> Result<PlanDocumentMetadata, PlanError> {
    record_query();
    let document: String = connection.query_row(
            "SELECT json_object('id', id, 'project_path',project_path,
                'canonical_project_path',coalesce(json_extract(header_json,'$.canonical_project_path'),''),
                'status',status) FROM plans WHERE id=?1",
            [id], |row| column(row,0),
        ).optional().map_err(db_error)?.ok_or(PlanError::NotFound)?;
    decode(id, &document)
}

mod manifest;
mod tags;

mod cleanup;
pub(super) mod regression;
mod team;
