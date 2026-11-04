use super::read_projections::{append_section, column, decode, record_query, SECTION_ROWS};
use super::{db_error, PlanDatabase};
use crate::plan::Plan;
use crate::service_plan::PlanError;
use rusqlite::OptionalExtension;

impl PlanDatabase {
    pub(in crate::service_plan) fn section_link_projection(
        &self,
        id: &str,
        unlinked_tasks: bool,
    ) -> Result<Plan, PlanError> {
        let mut connection = self.connection.lock();
        let tx = connection.transaction().map_err(db_error)?;
        record_query();
        let header: String=tx.query_row(
            "SELECT json_object('id',id,'title','','status',status,'owner',json('{}'),'project_path','','created_at',0,'updated_at',0)
             FROM plans WHERE id=?1",[id],|row|column(row,0),
        ).optional().map_err(db_error)?.ok_or(PlanError::NotFound)?;
        let mut plan: Plan = decode(id, &header)?;
        record_query();
        let mut statement=tx.prepare_cached(&format!(
            "SELECT path,json_object('id',id,'title',json_extract(document,'$.title'),'handle',json_extract(document,'$.handle'),
                'tags',json(CASE WHEN json_type(document,'$.tags') IS NULL THEN '[]' ELSE document->'$.tags' END))
             FROM ({SECTION_ROWS}) ORDER BY position,traversal"
        )).map_err(db_error)?;
        for row in statement
            .query_map([id], |row| {
                Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
            })
            .map_err(db_error)?
        {
            let (path, document) = row.map_err(db_error)?;
            append_section(
                &mut plan.sections,
                path.matches(".subsections[").count(),
                decode(id, &document)?,
            );
        }
        drop(statement);
        if unlinked_tasks {
            record_query();
            let mut statement=tx.prepare_cached(
                "SELECT json_object('id',json_extract(row_json,'$.id'),'name',json_extract(row_json,'$.name'),'type','','status','pending',
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
        tx.commit().map_err(db_error)?;
        Ok(plan)
    }
}
