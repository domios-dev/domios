use super::read_projections::{column, decode, record_query, SECTION_ROWS};
use super::{db_error, PlanDatabase};
use crate::service_plan::section_links::SectionLinkInput;
use crate::service_plan::PlanError;
use rusqlite::OptionalExtension;

impl PlanDatabase {
    pub(in crate::service_plan) fn section_link_projection(
        &self,
        id: &str,
        unlinked_tasks: bool,
    ) -> Result<SectionLinkInput, PlanError> {
        let mut connection = self.connection.lock();
        let tx = connection.transaction().map_err(db_error)?;
        record_query();
        tx.query_row("SELECT id FROM plans WHERE id=?1", [id], |row| {
            column::<String>(row, 0)
        })
        .optional()
        .map_err(db_error)?
        .ok_or(PlanError::NotFound)?;
        let plan = section_link_input(&tx, id, unlinked_tasks)?;
        tx.commit().map_err(db_error)?;
        Ok(plan)
    }
}

pub(super) fn section_link_input(
    tx: &rusqlite::Transaction<'_>,
    id: &str,
    unlinked_tasks: bool,
) -> Result<SectionLinkInput, PlanError> {
    let mut plan = SectionLinkInput {
        sections: Vec::new(),
        tasks: Vec::new(),
    };
    record_query();
    let mut statement=tx.prepare_cached(&format!(
            "SELECT json_object('id',id,'title',json_extract(document,'$.title'),'handle',json_extract(document,'$.handle'),
                'tags',json(CASE WHEN json_type(document,'$.tags') IS NULL THEN '[]' ELSE document->'$.tags' END))
             FROM ({SECTION_ROWS}) ORDER BY position,traversal"
        )).map_err(db_error)?;
    for row in statement
        .query_map([id], |row| column::<String>(row, 0))
        .map_err(db_error)?
    {
        plan.sections.push(decode(id, &row.map_err(db_error)?)?);
    }
    drop(statement);
    if unlinked_tasks {
        record_query();
        let mut statement=tx.prepare_cached(
                "SELECT json_object('id',json_extract(row_json,'$.id'),'name',json_extract(row_json,'$.name'),
                    'handle',json_extract(row_json,'$.handle'),'group',json_extract(row_json,'$.group'),
                    'section_ids',json(CASE WHEN json_type(row_json,'$.section_ids') IS NULL THEN '[]' ELSE row_json->'$.section_ids' END))
                 FROM plan_tasks WHERE plan_id=?1 AND coalesce(json_array_length(row_json,'$.section_ids'),0)=0 ORDER BY position"
            ).map_err(db_error)?;
        for document in statement
            .query_map([id], |row| column::<String>(row, 0))
            .map_err(db_error)?
        {
            plan.tasks.push(decode(id, &document.map_err(db_error)?)?);
        }
    }
    Ok(plan)
}
