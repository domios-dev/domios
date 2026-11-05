use super::read_projections::{column, decode, record_query, SECTION_ROWS};
use super::{db_error, PlanDatabase};
use crate::service_plan::search_index::{
    PlanSearchInput, PlanSearchSnapshot, SearchHeaderRead, SearchMilestoneRead, SearchSectionRead,
    SearchTaskRead,
};
use crate::service_plan::PlanError;
use rusqlite::OptionalExtension;

impl PlanDatabase {
    pub(in crate::service_plan) fn search_snapshot(
        &self,
        id: &str,
    ) -> Result<PlanSearchSnapshot, PlanError> {
        let mut connection = self.connection.lock();
        let tx = connection.transaction().map_err(db_error)?;
        record_query();
        let header: String=tx.query_row(
            "SELECT json_object('id',id,'title',title,'status',status,'project_path',project_path,
                'canonical_project_path',CASE WHEN json_type(header_json,'$.canonical_project_path') IS NULL THEN '' ELSE json_extract(header_json,'$.canonical_project_path') END,
                'updated_at',json(header_json->'$.updated_at')) FROM plans WHERE id=?1",
            [id],|row|column(row,0),
        ).optional().map_err(db_error)?.ok_or(PlanError::NotFound)?;
        let header: SearchHeaderRead = decode(id, &header)?;
        let mut plan = PlanSearchInput {
            header,
            sections: Vec::new(),
            tasks: Vec::new(),
        };
        record_query();
        let mut statement=tx.prepare_cached(&format!(
            "SELECT path,json_object('id',id,'title',json_extract(document,'$.title'),
                'body',CASE WHEN json_type(document,'$.body') IS NULL THEN '' ELSE json_extract(document,'$.body') END)
             FROM ({SECTION_ROWS}) ORDER BY position,traversal"
        )).map_err(db_error)?;
        for row in statement
            .query_map([id], |row| {
                Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
            })
            .map_err(db_error)?
        {
            let (path, document) = row.map_err(db_error)?;
            let section: SearchSectionRead = decode(id, &document)?;
            plan.sections
                .push((!path.contains(".subsections["), section));
        }
        drop(statement);
        record_query();
        let mut statement=tx.prepare_cached(
            "SELECT task_id,json_object('id',json_extract(row_json,'$.id'),'name',json_extract(row_json,'$.name'),
                'status',status,'updated_at',json(CASE WHEN json_type(row_json,'$.updated_at') IS NULL THEN '0' ELSE row_json->'$.updated_at' END),
                'body',CASE WHEN json_type(row_json,'$.body') IS NULL THEN '' ELSE json_extract(row_json,'$.body') END,
                'acceptance_criteria',json(CASE WHEN json_type(row_json,'$.acceptance_criteria') IS NULL THEN '[]' ELSE row_json->'$.acceptance_criteria' END),
                'external_refs',json(CASE WHEN json_type(row_json,'$.external_refs') IS NULL THEN '[]'
                    WHEN json_type(row_json,'$.external_refs')<>'array' THEN 'null' ELSE (SELECT json_group_array(json_extract(r.value,'$.canonical_url')) FROM json_each(t.row_json,'$.external_refs') r) END))
             FROM plan_tasks t WHERE plan_id=?1 ORDER BY position"
        ).map_err(db_error)?;
        let mut indexes = std::collections::HashMap::new();
        for row in statement
            .query_map([id], |row| {
                Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
            })
            .map_err(db_error)?
        {
            let (task_id, document) = row.map_err(db_error)?;
            indexes.insert(task_id, plan.tasks.len());
            plan.tasks.push(decode::<SearchTaskRead>(id, &document)?);
        }
        drop(statement);
        record_query();
        let mut statement=tx.prepare_cached(
            "SELECT m.task_id,json_object('id',json_extract(m.row_json,'$.id'),
                'kind',json_extract(m.row_json,'$.kind'),'text',CASE WHEN json_type(m.row_json,'$.text') IS NULL THEN '' ELSE json_extract(m.row_json,'$.text') END,
                'ts',json(m.row_json->'$.ts')) FROM plan_task_milestones m JOIN plan_tasks t USING(plan_id,task_id)
             WHERE m.plan_id=?1 ORDER BY t.position,m.seq"
        ).map_err(db_error)?;
        for row in statement
            .query_map([id], |row| {
                Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
            })
            .map_err(db_error)?
        {
            let (task_id, document) = row.map_err(db_error)?;
            if let Some(&index) = indexes.get(&task_id) {
                plan.tasks[index]
                    .milestones
                    .push(decode::<SearchMilestoneRead>(id, &document)?)
            }
        }
        drop(statement);
        tx.commit().map_err(db_error)?;
        drop(connection);
        Ok(PlanSearchSnapshot::from_input(plan))
    }
}
